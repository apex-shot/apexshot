use std::sync::atomic::{AtomicU64, Ordering};

/// When the daemon is not running, the overlay starts its own PipeWire audio
/// level monitoring and stores the results here.
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

/// Start local PipeWire audio-level monitoring for the overlay.
///
/// Used when the daemon is not running (standalone capture mode on
/// compositors like Hyprland). Spawns two PipeWire capture streams
/// (mic + system audio) only while the recording panel needs meters so we
/// do not force Bluetooth headsets into HSP/HFP mode (issue #41).
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
            eprintln!("[overlay] PipeWire: releasing local audio meters");
            stop.store(true, Ordering::Release);
        }
    }
    OVERLAY_MIC_LEVEL.store(0.0f64.to_bits(), Ordering::Relaxed);
    OVERLAY_SPEAKER_LEVEL.store(0.0f64.to_bits(), Ordering::Relaxed);
}

/// Spawn a single PipeWire capture stream for the overlay's audio meter.
pub(super) fn spawn_overlay_pw_stream(
    label: &'static str,
    stream_name: &'static str,
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
                eprintln!("[overlay] PipeWire ({}) monitoring stopped.", self.label);
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
        std::thread::sleep(std::time::Duration::from_millis(200));
        if stop.load(Ordering::Acquire) {
            return;
        }

        use pipewire as pw;
        use pw::{properties::properties, spa};
        use spa::param::format::{MediaSubtype, MediaType};
        use spa::param::format_utils;

        pw::init();

        let mainloop = match pw::main_loop::MainLoopRc::new(None) {
            Ok(ml) => ml,
            Err(e) => {
                eprintln!("[overlay] PipeWire ({label}): main loop: {e}");
                return;
            }
        };
        let context = match pw::context::ContextRc::new(&mainloop, None) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("[overlay] PipeWire ({label}): context: {e}");
                return;
            }
        };
        let core = match context.connect_rc(None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[overlay] PipeWire ({label}): core: {e}");
                return;
            }
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
                eprintln!("[overlay] PipeWire ({label}): stream: {e}");
                return;
            }
        };

        let _listener = stream
            .add_local_listener_with_user_data(spa::param::audio::AudioInfoRaw::default())
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
                user_data.parse(param).ok();
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

                let rms = if count > 0 {
                    (sum_sq / count as f64).sqrt()
                } else {
                    0.0
                };
                let raw_level = (rms * 3.0).clamp(0.0, 1.0);

                let gated = if !capture_sink && raw_level < 0.15 {
                    0.0
                } else {
                    raw_level
                };

                level.store(gated.to_bits(), Ordering::Relaxed);
            })
            .register();

        // Build F32LE mono format
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
            Ok(_) => eprintln!("[overlay] PipeWire ({label}) monitoring started."),
            Err(e) => {
                eprintln!("[overlay] PipeWire ({label}): connect: {e}");
                return;
            }
        }

        let stop_for_timer = stop.clone();
        let mainloop_for_timer = mainloop.clone();
        let stop_timer = mainloop.loop_().add_timer(move |_| {
            if stop_for_timer.load(Ordering::Acquire) {
                mainloop_for_timer.quit();
            }
        });
        let _ = stop_timer.update_timer(
            Some(std::time::Duration::from_millis(250)),
            Some(std::time::Duration::from_millis(250)),
        );

        mainloop.run();
    });
}

pub(super) fn set_mic_volume(vol: f64) {
    let pct = (vol.clamp(0.0, 1.0) * 100.0).round() as u32;
    std::thread::spawn(move || {
        let _ = std::process::Command::new("pactl")
            .args([
                "set-source-volume",
                "@DEFAULT_SOURCE@",
                &format!("{}%", pct),
            ])
            .output();
    });
}

pub(super) fn set_speaker_volume(vol: f64) {
    let pct = (vol.clamp(0.0, 1.0) * 100.0).round() as u32;
    std::thread::spawn(move || {
        let _ = std::process::Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", pct)])
            .output();
    });
}
