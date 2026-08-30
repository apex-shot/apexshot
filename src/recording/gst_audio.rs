//! Unified GStreamer audio capture for recordings.
//!
//! One programmatically-built bin mixes the microphone and speaker-monitor
//! Pulse sources and encodes the mix for the active video muxer:
//!
//! ```text
//! pulsesrc(mic)      ! queue ! volume ! audioconvert ! audioresample ! caps ! audiomixer
//! pulsesrc(monitor) ! queue ! volume ! audioconvert ! audioresample ! caps ! audiomixer
//! audiomixer ! queue ! audioconvert ! caps ! encoder ! aacparse|oggmux ! appsink|ghost pad
//! ```
//!
//! Wayland recordings hand the encoded frames to ffmpeg through an inherited
//! pipe fd (`-f aac|ogg -i pipe:3 -c:a copy`); X11 links the ghost pad into
//! the existing GStreamer muxer via a request pad.

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use super::audio::{PulseMicDevice, ResolvedAudioDevices};
use super::RecordingConfig;

pub(super) const AUDIO_SAMPLE_RATE: i32 = 48000;
/// Real mics (especially after the overlay meter releases the source) need
/// longer than a testsrc. Keep this bounded so a dead source still fails.
const FIRST_SAMPLE_GRACE: Duration = Duration::from_secs(2);
/// Pulse buffer (µs). Loud transients (snaps) xrun a 10ms fragment and the
/// mixer then inserts digital silence — keep enough cushion for peaks.
const PULSE_BUFFER_TIME_US: i64 = 400_000;
const PULSE_LATENCY_TIME_US: i64 = 40_000;
/// Branch/output queues (ns). Leak the oldest buffers instead of stalling
/// pulsesrc when the encoder or ffmpeg pipe is briefly busy.
const AUDIO_QUEUE_TIME_NS: u64 = 200_000_000;

/// Process-wide GStreamer init. Must run before any `pw::init()` in this
/// process so gstpulse and the native PipeWire video thread do not fight
/// over spa/libpipewire startup.
pub(crate) fn ensure_gst_initialized() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    match INIT.get_or_init(|| gst::init().map_err(|err| format!("GStreamer init failed: {err}"))) {
        Ok(()) => Ok(()),
        Err(err) => Err(err.clone()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioEncoder {
    Aac,
    Opus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioTermination {
    /// Encoded frames end in an appsink pulled by a Rust writer thread.
    AppSink,
    /// The bin exposes a ghost src pad to link into a video muxer.
    GhostPad,
}

pub(super) fn encoder_for_muxer(muxer: &str) -> Option<AudioEncoder> {
    match muxer {
        "mp4mux" | "qtmux" => Some(AudioEncoder::Aac),
        "webmmux" | "matroskamux" | "oggmux" => Some(AudioEncoder::Opus),
        _ => None,
    }
}

/// ffmpeg demuxer name for the encoded audio stream on the inherited pipe.
pub(super) fn ffmpeg_input_format(encoder: AudioEncoder) -> &'static str {
    match encoder {
        AudioEncoder::Aac => "aac",
        AudioEncoder::Opus => "ogg",
    }
}

fn encoder_element_factory(encoder: AudioEncoder) -> Option<&'static str> {
    let candidates: &[&str] = match encoder {
        AudioEncoder::Aac => &["avenc_aac", "voaacenc", "faac"],
        AudioEncoder::Opus => &["opusenc"],
    };
    candidates
        .iter()
        .copied()
        .find(|name| gst::ElementFactory::find(name).is_some())
}

fn required_elements(
    encoder: AudioEncoder,
    termination: AudioTermination,
    noise_suppression: bool,
) -> Option<Vec<&'static str>> {
    let mut elements = vec![
        "pulsesrc",
        "audiomixer",
        "audioconvert",
        "audioresample",
        "queue",
        "volume",
    ];
    elements.push(encoder_element_factory(encoder)?);
    match encoder {
        AudioEncoder::Aac => elements.push("aacparse"),
        AudioEncoder::Opus => {
            if matches!(termination, AudioTermination::AppSink) {
                elements.push("oggmux");
            }
        }
    }
    if noise_suppression {
        elements.push("webrtcdsp");
    }
    Some(elements)
}

/// Cheap pre-flight probe: can this machine build the audio bin at all?
pub(super) fn audio_available(
    muxer: &str,
    termination: AudioTermination,
    noise_suppression: bool,
) -> bool {
    if ensure_gst_initialized().is_err() {
        return false;
    }
    let Some(encoder) = encoder_for_muxer(muxer) else {
        return false;
    };
    required_elements(encoder, termination, noise_suppression)
        .map(|elements| {
            elements
                .iter()
                .all(|name| gst::ElementFactory::find(name).is_some())
        })
        .unwrap_or(false)
}

/// Inputs for the audio bin, resolved from a `RecordingConfig`.
#[derive(Debug, Clone)]
pub(super) struct GstAudioSetup {
    /// Mic branch input; `None` disables the branch.
    pub mic: Option<PulseMicDevice>,
    /// Speaker-monitor branch input (a real `.monitor` source name).
    pub monitor: Option<String>,
    pub mono: bool,
    pub noise_suppression: bool,
    /// Replace pulsesrc with this element factory (tests use `audiotestsrc`).
    pub source_factory_override: Option<&'static str>,
    /// Pulse client name so PipeWire can tell the meter apart from recording.
    pub client_name: String,
}

impl GstAudioSetup {
    /// Resolve devices for GStreamer capture. `None` when audio is disabled or
    /// the monitor source name cannot be resolved for pulsesrc.
    pub(super) fn from_recording(config: &RecordingConfig) -> Option<Self> {
        if !config.mic_enabled && !config.speaker_enabled {
            return None;
        }
        let ResolvedAudioDevices { mic, monitor } =
            super::audio::resolve_recording_audio_devices(config)?;
        Some(Self {
            mic,
            monitor,
            mono: config.mono_audio,
            noise_suppression: config.noise_suppression,
            source_factory_override: None,
            client_name: "ApexShot Recording".into(),
        })
    }
}

pub(super) struct GstAudioBin {
    pub bin: gst::Bin,
    pub appsink: Option<gst_app::AppSink>,
    pub volumes: Vec<gst::Element>,
}

impl GstAudioBin {
    /// Mute/unmute one capture branch (mic = 0, monitor = 1 when present).
    /// The overlay volume sliders keep mutating system volume (WYSIWYG
    /// recording); this handle is for programmatic mute from control plumbing.
    #[allow(dead_code)]
    pub fn set_branch_muted(&self, branch: usize, muted: bool) {
        if let Some(volume) = self.volumes.get(branch) {
            volume.set_property("volume", if muted { 0.0f64 } else { 1.0f64 });
        }
    }
}

fn make_element(factory: &str, name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .map_err(|_| format!("missing GStreamer element '{factory}'"))
}

fn branch_caps(channels: i32) -> gst::Caps {
    gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("rate", AUDIO_SAMPLE_RATE)
        .field("channels", channels)
        .field("layout", "interleaved")
        .build()
}

fn encoder_caps(encoder: AudioEncoder, channels: i32) -> gst::Caps {
    let format = match encoder {
        AudioEncoder::Aac => "F32LE",
        AudioEncoder::Opus => "S16LE",
    };
    gst::Caps::builder("audio/x-raw")
        .field("format", format)
        .field("rate", AUDIO_SAMPLE_RATE)
        .field("channels", channels)
        .field("layout", "interleaved")
        .build()
}

fn make_source(
    setup: &GstAudioSetup,
    name: &str,
    device: Option<&str>,
) -> Result<gst::Element, String> {
    if let Some(factory) = setup.source_factory_override {
        return gst::ElementFactory::make(factory)
            .name(name)
            .property("is-live", true)
            .build()
            .map_err(|_| format!("missing test source '{factory}'"));
    }
    let builder = gst::ElementFactory::make("pulsesrc").name(name);
    let builder = match device {
        Some(device) => builder.property("device", device),
        None => builder,
    };
    let src = builder
        .build()
        .map_err(|_| "missing GStreamer element 'pulsesrc'".to_string())?;
    if src.find_property("client-name").is_some() {
        src.set_property("client-name", setup.client_name.as_str());
    }
    // Two live pulsesrc clocks (or a meter + capture on the same device)
    // fight and drop ~60ms of zeros after peaks. Slave to the pipeline clock
    // and request a larger Pulse fragment so snaps don't xrun.
    if src.find_property("provide-clock").is_some() {
        src.set_property("provide-clock", false);
    }
    if src.find_property("do-timestamp").is_some() {
        src.set_property("do-timestamp", true);
    }
    if src.find_property("buffer-time").is_some() {
        src.set_property("buffer-time", PULSE_BUFFER_TIME_US);
    }
    if src.find_property("latency-time").is_some() {
        src.set_property("latency-time", PULSE_LATENCY_TIME_US);
    }
    Ok(src)
}

fn configure_live_queue(queue: &gst::Element) {
    if queue.find_property("leaky").is_some() {
        queue.set_property_from_str("leaky", "downstream");
    }
    queue.set_property("max-size-buffers", 0u32);
    queue.set_property("max-size-bytes", 0u32);
    queue.set_property("max-size-time", AUDIO_QUEUE_TIME_NS);
}

fn build_branch(
    setup: &GstAudioSetup,
    prefix: &str,
    device: Option<&str>,
    with_dsp: bool,
    channels: i32,
) -> Result<Vec<gst::Element>, String> {
    let queue = make_element("queue", &format!("{prefix}_queue"))?;
    configure_live_queue(&queue);
    let mut elements = vec![
        make_source(setup, &format!("{prefix}_src"), device)?,
        queue,
        make_element("volume", &format!("{prefix}_volume"))?,
        make_element("audioconvert", &format!("{prefix}_convert"))?,
        make_element("audioresample", &format!("{prefix}_resample"))?,
    ];
    if with_dsp {
        let dsp_caps = make_element("capsfilter", &format!("{prefix}_dsp_caps"))?;
        dsp_caps.set_property("caps", branch_caps(channels));
        let dsp = make_element("webrtcdsp", &format!("{prefix}_webrtcdsp"))?;
        dsp.set_property("noise-suppression", true);
        dsp.set_property("echo-cancel", false);
        elements.push(dsp_caps);
        elements.push(dsp);
        elements.push(make_element(
            "audioconvert",
            &format!("{prefix}_convert_post_dsp"),
        )?);
        elements.push(make_element(
            "audioresample",
            &format!("{prefix}_resample_post_dsp"),
        )?);
    }
    let caps = make_element("capsfilter", &format!("{prefix}_caps"))?;
    caps.set_property("caps", branch_caps(channels));
    elements.push(caps);
    Ok(elements)
}

/// Build the shared audio bin. Elements are created programmatically (not
/// parse_launch) so pause/mute/EOS can be controlled at runtime.
pub(super) fn build_audio_bin(
    setup: &GstAudioSetup,
    encoder: AudioEncoder,
    termination: AudioTermination,
) -> Result<GstAudioBin, String> {
    let channels = if setup.mono { 1 } else { 2 };
    let bin = gst::Bin::new();

    let mut volumes = Vec::new();
    let mut branches: Vec<Vec<gst::Element>> = Vec::new();
    if let Some(mic) = setup.mic.as_ref() {
        let device = match mic {
            PulseMicDevice::Default => None,
            PulseMicDevice::Named(name) => Some(name.as_str()),
        };
        branches.push(build_branch(
            setup,
            "mic",
            device,
            setup.noise_suppression,
            channels,
        )?);
    }
    if let Some(monitor) = setup.monitor.as_ref() {
        branches.push(build_branch(
            setup,
            "mon",
            Some(monitor.as_str()),
            false,
            channels,
        )?);
    }
    if branches.is_empty() {
        return Err("audio bin requested without any capture branch".into());
    }

    let out_queue = make_element("queue", "audio_out_queue")?;
    configure_live_queue(&out_queue);
    let mut out_elements = vec![
        out_queue,
        make_element("audioconvert", "audio_out_convert")?,
    ];
    let enc_caps = make_element("capsfilter", "audio_enc_caps")?;
    enc_caps.set_property("caps", encoder_caps(encoder, channels));
    out_elements.push(enc_caps);
    let encoder_name = encoder_element_factory(encoder)
        .ok_or_else(|| format!("no GStreamer encoder for {encoder:?}"))?;
    out_elements.push(make_element(encoder_name, "audio_encoder")?);

    match (encoder, termination) {
        (AudioEncoder::Aac, AudioTermination::AppSink) => {
            let parse = make_element("aacparse", "audio_aacparse")?;
            let adts_caps = make_element("capsfilter", "audio_adts_caps")?;
            adts_caps.set_property(
                "caps",
                gst::Caps::builder("audio/mpeg")
                    .field("stream-format", "adts")
                    .build(),
            );
            out_elements.push(parse);
            out_elements.push(adts_caps);
        }
        (AudioEncoder::Aac, AudioTermination::GhostPad) => {
            let parse = make_element("aacparse", "audio_aacparse")?;
            let raw_caps = make_element("capsfilter", "audio_raw_caps")?;
            raw_caps.set_property(
                "caps",
                gst::Caps::builder("audio/mpeg")
                    .field("mpegversion", 4)
                    .field("stream-format", "raw")
                    .field("framed", true)
                    .build(),
            );
            out_elements.push(parse);
            out_elements.push(raw_caps);
        }
        (AudioEncoder::Opus, AudioTermination::AppSink) => {
            out_elements.push(make_element("oggmux", "audio_oggmux")?);
        }
        (AudioEncoder::Opus, AudioTermination::GhostPad) => {}
    }

    // A single live pad through audiomixer still aligns on running time and
    // inserts digital silence after a timestamp jump (exactly the snap crack).
    let mixer = if branches.len() > 1 {
        let mixer = make_element("audiomixer", "audio_mix")?;
        if mixer.find_property("ignore-inactive-pads").is_some() {
            mixer.set_property("ignore-inactive-pads", true);
        }
        Some(mixer)
    } else {
        None
    };

    let mut all_elements: Vec<gst::Element> = Vec::new();
    if let Some(mixer) = mixer.as_ref() {
        all_elements.push(mixer.clone());
    }
    for branch in &branches {
        for element in branch {
            if element.factory().is_some_and(|f| f.name() == "volume") {
                volumes.push(element.clone());
            }
        }
        all_elements.extend(branch.iter().cloned());
    }
    all_elements.extend(out_elements.iter().cloned());
    bin.add_many(&all_elements)
        .map_err(|err| format!("failed to add audio elements: {err}"))?;

    for branch in &branches {
        for pair in branch.windows(2) {
            pair[0]
                .link(&pair[1])
                .map_err(|err| format!("failed to link audio branch: {err}"))?;
        }
        if let Some(mixer) = mixer.as_ref() {
            let sink_pad = mixer
                .request_pad_simple("sink_%u")
                .ok_or("failed to request audiomixer sink pad")?;
            branch
                .last()
                .expect("branch is never empty")
                .static_pad("src")
                .ok_or("audio branch has no src pad")?
                .link(&sink_pad)
                .map_err(|err| format!("failed to link branch into audiomixer: {err:?}"))?;
        }
    }

    let out_sink = out_elements[0]
        .static_pad("sink")
        .ok_or("audio output queue has no sink pad")?;
    for pair in out_elements.windows(2) {
        pair[0]
            .link(&pair[1])
            .map_err(|err| format!("failed to link audio output chain: {err}"))?;
    }
    let mix_src = if let Some(mixer) = mixer.as_ref() {
        mixer.static_pad("src").ok_or("audiomixer has no src pad")?
    } else {
        branches[0]
            .last()
            .expect("branch is never empty")
            .static_pad("src")
            .ok_or("audio branch has no src pad")?
    };
    mix_src
        .link(&out_sink)
        .map_err(|err| format!("failed to link audio into encoder: {err:?}"))?;

    let mut appsink = None;
    let last = out_elements.last().ok_or("audio output chain is empty")?;
    match termination {
        AudioTermination::AppSink => {
            let sink = gst_app::AppSink::builder()
                .name("audio_encoded_sink")
                .sync(false)
                .max_buffers(256)
                .drop(false)
                .build();
            bin.add(&sink)
                .map_err(|err| format!("failed to add appsink: {err}"))?;
            last.link(&sink)
                .map_err(|err| format!("failed to link appsink: {err}"))?;
            appsink = Some(sink);
        }
        AudioTermination::GhostPad => {
            let target = last
                .static_pad("src")
                .ok_or("audio output has no src pad for ghosting")?;
            let ghost = gst::GhostPad::builder_with_target(&target)
                .map_err(|err| format!("failed to create audio ghost pad: {err}"))?
                .name("src")
                .build();
            bin.add_pad(&ghost)
                .map_err(|err| format!("failed to add audio ghost pad: {err}"))?;
            ghost
                .set_active(true)
                .map_err(|err| format!("failed to activate audio ghost pad: {err}"))?;
        }
    }

    Ok(GstAudioBin {
        bin,
        appsink,
        volumes,
    })
}

fn create_pipe() -> Result<(OwnedFd, OwnedFd), String> {
    let mut fds = [0i32; 2];
    // O_CLOEXEC keeps the ffmpeg child from inheriting stray pipe ends: the
    // child must never hold a write end (its own reads would never EOF) and
    // fd 3 is attached explicitly in the pre_exec hook.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!(
            "failed to create audio pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn append_sample_bytes(sample: &gst::Sample, out: &mut Vec<u8>) {
    if let Some(buffer) = sample.buffer() {
        if let Ok(map) = buffer.map_readable() {
            out.extend_from_slice(map.as_slice());
        }
    }
}

/// A running Wayland audio capture: gst pipeline + writer thread feeding the
/// inherited pipe that ffmpeg reads as its second input.
pub(super) struct ActiveGstAudio {
    pipeline: gst::Pipeline,
    encoder: AudioEncoder,
    read_fd: Option<OwnedFd>,
    halt: Arc<AtomicBool>,
    done: Option<mpsc::Receiver<()>>,
    writer: Option<std::thread::JoinHandle<()>>,
    paused: bool,
}

impl ActiveGstAudio {
    /// Build and start the audio pipeline. Waits for the first encoded sample
    /// (bounded by a short grace window) so a failing source is detected
    /// before ffmpeg is spawned with the audio pipe attached — ffmpeg
    /// hard-fails on an empty audio input.
    pub(super) fn start(setup: &GstAudioSetup, muxer: &str) -> Result<Self, String> {
        ensure_gst_initialized()?;
        let encoder =
            encoder_for_muxer(muxer).ok_or_else(|| format!("no audio encoder for '{muxer}'"))?;
        let bin = build_audio_bin(setup, encoder, AudioTermination::AppSink)?;
        let appsink = bin.appsink.clone().ok_or("audio bin has no appsink")?;
        let pipeline = gst::Pipeline::new();
        pipeline
            .add(&bin.bin)
            .map_err(|err| format!("failed to add audio bin: {err}"))?;

        let (read_fd, write_fd) = create_pipe()?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|err| format!("failed to start audio pipeline: {err:?}"))?;

        let bus = pipeline.bus().ok_or("audio pipeline has no bus")?;
        let deadline = Instant::now() + FIRST_SAMPLE_GRACE;
        let mut prebuf = Vec::new();
        loop {
            for message in bus.iter_timed(gst::ClockTime::ZERO) {
                if let gst::MessageView::Error(err) = message.view() {
                    let _ = pipeline.set_state(gst::State::Null);
                    return Err(format!("audio pipeline error: {}", err.error()));
                }
            }
            if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::ZERO) {
                append_sample_bytes(&sample, &mut prebuf);
                break;
            }
            if Instant::now() > deadline {
                let _ = pipeline.set_state(gst::State::Null);
                return Err("audio pipeline produced no encoded data".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let halt = Arc::new(AtomicBool::new(false));
        let writer_halt = halt.clone();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let writer = std::thread::Builder::new()
            .name("apexshot-audio-writer".into())
            .spawn(move || {
                let done_tx = done_tx;
                let mut file: std::fs::File = write_fd.into();
                if !prebuf.is_empty() {
                    if let Err(err) = file.write_all(&prebuf) {
                        eprintln!("[recording] audio pipe write failed: {err}");
                        let _ = done_tx.send(());
                        return;
                    }
                }
                while !writer_halt.load(Ordering::Acquire) {
                    match appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                        Some(sample) => {
                            let mut bytes = Vec::new();
                            append_sample_bytes(&sample, &mut bytes);
                            if let Err(err) = file.write_all(&bytes) {
                                if err.kind() != std::io::ErrorKind::BrokenPipe {
                                    eprintln!("[recording] audio pipe write failed: {err}");
                                }
                                break;
                            }
                        }
                        None => {
                            if appsink.is_eos() {
                                break;
                            }
                        }
                    }
                }
                let _ = done_tx.send(());
            })
            .map_err(|err| format!("failed to spawn audio writer: {err}"))?;

        Ok(Self {
            pipeline,
            encoder,
            read_fd: Some(read_fd),
            halt,
            done: Some(done_rx),
            writer: Some(writer),
            paused: false,
        })
    }

    /// The pipe fd ffmpeg should inherit as fd 3.
    pub(super) fn take_read_fd(&mut self) -> Option<OwnedFd> {
        self.read_fd.take()
    }

    pub(super) fn input_format(&self) -> &'static str {
        ffmpeg_input_format(self.encoder)
    }

    /// Pause the capture without un-linking anything. Pipeline state is used
    /// (rather than dropping buffers) so buffer timestamps stay continuous
    /// across the pause and audio never drifts against the video timeline.
    pub(super) fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        let state = if paused {
            gst::State::Paused
        } else {
            gst::State::Playing
        };
        if let Err(err) = self.pipeline.set_state(state) {
            eprintln!("[recording] audio pause/resume failed: {err:?}");
            return;
        }
        self.paused = paused;
    }

    /// Drain the bus and return the first pipeline error, if any.
    pub(super) fn poll_bus_error(&self) -> Option<String> {
        let bus = self.pipeline.bus()?;
        for message in bus.iter_timed(gst::ClockTime::ZERO) {
            if let gst::MessageView::Error(err) = message.view() {
                return Some(err.error().to_string());
            }
        }
        None
    }

    /// Stop cleanly: EOS the pipeline, let the writer drain remaining frames,
    /// then close the pipe (ffmpeg finalizes once its audio input EOFs).
    pub(super) fn stop(mut self) {
        if self.paused {
            let _ = self.pipeline.set_state(gst::State::Playing);
        }
        self.pipeline.send_event(gst::event::Eos::new());
        self.join_writer(Duration::from_secs(5));
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    /// Stop after a pipeline error: no EOS wait, just halt the writer.
    pub(super) fn abort(&mut self) {
        self.halt.store(true, Ordering::Release);
        self.join_writer(Duration::from_secs(2));
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    fn join_writer(&mut self, timeout: Duration) {
        let Some(done) = self.done.take() else {
            return;
        };
        match done.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                eprintln!("[recording] audio writer did not finish in time; halting it");
                self.halt.store(true, Ordering::Release);
                let _ = done.recv_timeout(Duration::from_secs(2));
            }
        }
        match self.writer.take() {
            Some(writer) if writer.is_finished() => {
                let _ = writer.join();
            }
            // A writer stuck in a blocked write (ffmpeg hung) is left to exit
            // on its own once ffmpeg is killed and the pipe breaks.
            Some(writer) => drop(writer),
            None => {}
        }
    }
}

/// Overlay / daemon mic-level meter on the same pulsesrc stack as recording.
///
/// Native `pw::init()` meters in this process race GStreamer and the
/// PipeWire video thread. Driving the meter through pulsesrc means
/// `gst::init()` happens first and recording can take the device cleanly.
pub(crate) fn run_pulse_level_monitor(
    label: &'static str,
    device: Option<String>,
    capture_sink: bool,
    level: &'static AtomicU64,
    stop: Arc<AtomicBool>,
) {
    if let Err(err) = ensure_gst_initialized() {
        eprintln!("[audio] {label} meter: {err}");
        return;
    }
    if gst::ElementFactory::find("pulsesrc").is_none() {
        eprintln!("[audio] {label} meter: pulsesrc missing");
        return;
    }

    let device = match (device, capture_sink) {
        (Some(name), _) => Some(name),
        (None, true) => {
            let name = super::audio::get_pulse_speaker_monitor();
            if name == "default.monitor" {
                None
            } else {
                Some(name)
            }
        }
        (None, false) => None,
    };

    let src = match gst::ElementFactory::make("pulsesrc")
        .name(format!("{label}_meter_src"))
        .build()
    {
        Ok(src) => src,
        Err(_) => {
            eprintln!("[audio] {label} meter: failed to create pulsesrc");
            return;
        }
    };
    if let Some(ref device) = device {
        src.set_property("device", device.as_str());
    }
    if src.find_property("client-name").is_some() {
        src.set_property("client-name", format!("ApexShot {label} meter"));
    }

    let convert = match make_element("audioconvert", &format!("{label}_meter_convert")) {
        Ok(el) => el,
        Err(err) => {
            eprintln!("[audio] {label} meter: {err}");
            return;
        }
    };
    let resample = match make_element("audioresample", &format!("{label}_meter_resample")) {
        Ok(el) => el,
        Err(err) => {
            eprintln!("[audio] {label} meter: {err}");
            return;
        }
    };
    let capsfilter = match make_element("capsfilter", &format!("{label}_meter_caps")) {
        Ok(el) => el,
        Err(err) => {
            eprintln!("[audio] {label} meter: {err}");
            return;
        }
    };
    capsfilter.set_property("caps", branch_caps(1));
    let appsink = gst_app::AppSink::builder()
        .name(format!("{label}_meter_sink"))
        .sync(false)
        .max_buffers(8)
        .drop(true)
        .build();

    let pipeline = gst::Pipeline::new();
    if pipeline
        .add_many([&src, &convert, &resample, &capsfilter, appsink.upcast_ref()])
        .is_err()
    {
        eprintln!("[audio] {label} meter: failed to add elements");
        return;
    }
    if gst::Element::link_many([&src, &convert, &resample, &capsfilter, appsink.upcast_ref()])
        .is_err()
    {
        eprintln!("[audio] {label} meter: failed to link elements");
        let _ = pipeline.set_state(gst::State::Null);
        return;
    }
    if pipeline.set_state(gst::State::Playing).is_err() {
        eprintln!("[audio] {label} meter: failed to play");
        let _ = pipeline.set_state(gst::State::Null);
        return;
    }
    eprintln!("[audio] {label} meter: GStreamer pulsesrc started");

    while !stop.load(Ordering::Acquire) {
        if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(80)) {
            if let Some(buffer) = sample.buffer() {
                if let Ok(map) = buffer.map_readable() {
                    let bytes = map.as_slice();
                    let mut sum_sq = 0.0f64;
                    let mut count = 0u64;
                    for chunk in bytes.chunks_exact(2) {
                        let s = i16::from_le_bytes([chunk[0], chunk[1]]) as f64 / 32768.0;
                        sum_sq += s * s;
                        count += 1;
                    }
                    if count > 0 {
                        let rms = (sum_sq / count as f64).sqrt();
                        let meter = crate::daemon::audio_meter_level(rms, capture_sink);
                        level.store(meter.to_bits(), Ordering::Relaxed);
                    }
                }
            }
        }
    }

    let _ = pipeline.set_state(gst::State::Null);
    level.store(0.0f64.to_bits(), Ordering::Relaxed);
    eprintln!("[audio] {label} meter: stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn wait_for_ffmpeg_or_kill(
        child: &mut std::process::Child,
        timeout: Duration,
    ) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Ok(None) => {
                    let _ = child.kill();
                    return child.wait().expect("ffmpeg should reap after kill");
                }
                Err(err) => panic!("failed to wait for ffmpeg: {err}"),
            }
        }
    }

    fn skip_if_gst_audio_unavailable(encoder: AudioEncoder, app_sink: bool) -> bool {
        if ensure_gst_initialized().is_err() {
            eprintln!("skipping: GStreamer not available");
            return true;
        }
        let mut needed: Vec<&str> = vec!["audiotestsrc", "audiomixer", "audioconvert", "volume"];
        match encoder_element_factory(encoder) {
            Some(name) => needed.push(name),
            None => {
                eprintln!("skipping: GStreamer audio encoder missing");
                return true;
            }
        }
        match encoder {
            AudioEncoder::Aac => needed.push("aacparse"),
            AudioEncoder::Opus => {
                if app_sink {
                    needed.push("oggmux");
                }
            }
        }
        if needed
            .iter()
            .any(|name| gst::ElementFactory::find(name).is_none())
        {
            eprintln!("skipping: GStreamer audio elements missing");
            return true;
        }
        false
    }

    fn test_setup(mono: bool) -> GstAudioSetup {
        GstAudioSetup {
            mic: Some(PulseMicDevice::Default),
            monitor: Some("test.monitor".to_string()),
            mono,
            noise_suppression: false,
            source_factory_override: Some("audiotestsrc"),
            client_name: "ApexShot Test".into(),
        }
    }

    #[test]
    fn encoder_mapping_covers_every_recording_muxer() {
        assert_eq!(encoder_for_muxer("mp4mux"), Some(AudioEncoder::Aac));
        assert_eq!(encoder_for_muxer("qtmux"), Some(AudioEncoder::Aac));
        assert_eq!(encoder_for_muxer("webmmux"), Some(AudioEncoder::Opus));
        assert_eq!(encoder_for_muxer("oggmux"), Some(AudioEncoder::Opus));
        assert_eq!(encoder_for_muxer("matroskamux"), Some(AudioEncoder::Opus));
        assert_eq!(encoder_for_muxer("unknownmux"), None);
        assert_eq!(ffmpeg_input_format(AudioEncoder::Aac), "aac");
        assert_eq!(ffmpeg_input_format(AudioEncoder::Opus), "ogg");
    }

    #[test]
    fn availability_probe_rejects_unknown_muxers_without_gst() {
        assert!(!audio_available(
            "nonexistent-muxer",
            AudioTermination::AppSink,
            false
        ));
    }

    #[test]
    fn audio_bin_links_and_produces_encoded_frames() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        for (muxer, encoder) in [
            ("mp4mux", AudioEncoder::Aac),
            ("webmmux", AudioEncoder::Opus),
        ] {
            let setup = test_setup(false);
            let bin = build_audio_bin(&setup, encoder, AudioTermination::AppSink)
                .unwrap_or_else(|err| panic!("{muxer}: {err}"));
            let appsink = bin.appsink.as_ref().unwrap().clone();
            let pipeline = gst::Pipeline::new();
            pipeline.add(&bin.bin).unwrap();
            pipeline.set_state(gst::State::Playing).unwrap();

            let sample = appsink
                .try_pull_sample(gst::ClockTime::from_seconds(3))
                .unwrap_or_else(|| panic!("{muxer} produced no encoded audio"));
            let buffer = sample.buffer().expect("sample has a buffer");
            assert!(buffer.size() > 0);

            pipeline.send_event(gst::event::Eos::new());
            let _ = pipeline.set_state(gst::State::Null);
        }
    }

    #[test]
    fn mono_config_builds_single_channel_branches() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        let setup = test_setup(true);
        let bin = build_audio_bin(&setup, AudioEncoder::Aac, AudioTermination::AppSink).unwrap();
        for name in ["mic_caps", "mon_caps"] {
            let capsfilter = bin
                .bin
                .by_name(name)
                .unwrap_or_else(|| panic!("{name} missing"));
            let caps: gst::Caps = capsfilter.property_value("caps").get().unwrap();
            let structure = caps.structure(0).unwrap();
            assert_eq!(structure.get::<i32>("channels").unwrap(), 1);
            assert_eq!(structure.get::<i32>("rate").unwrap(), AUDIO_SAMPLE_RATE);
        }
        let _ = bin.bin.set_state(gst::State::Null);
    }

    #[test]
    fn mic_only_bin_skips_audiomixer() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        let mut setup = test_setup(false);
        setup.monitor = None;
        let bin = build_audio_bin(&setup, AudioEncoder::Aac, AudioTermination::AppSink).unwrap();
        assert!(
            bin.bin.by_name("audio_mix").is_none(),
            "a single capture branch must not go through audiomixer"
        );
        assert_eq!(bin.volumes.len(), 1);
        let _ = bin.bin.set_state(gst::State::Null);
    }

    #[test]
    fn two_branch_bin_keeps_mixer_and_ignores_inactive_pads() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        let setup = test_setup(false);
        let bin = build_audio_bin(&setup, AudioEncoder::Aac, AudioTermination::AppSink).unwrap();
        let mixer = bin
            .bin
            .by_name("audio_mix")
            .expect("mic+monitor still uses audiomixer");
        if mixer.find_property("ignore-inactive-pads").is_some() {
            assert!(mixer.property::<bool>("ignore-inactive-pads"));
        }
        assert_eq!(bin.volumes.len(), 2);
        let _ = bin.bin.set_state(gst::State::Null);
    }

    #[test]
    fn audio_bin_exposes_branch_mute_handles() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        let setup = test_setup(false);
        let bin = build_audio_bin(&setup, AudioEncoder::Aac, AudioTermination::AppSink).unwrap();
        assert_eq!(bin.volumes.len(), 2);
        bin.set_branch_muted(0, true);
        assert_eq!(bin.volumes[0].property::<f64>("volume"), 0.0);
        bin.set_branch_muted(0, false);
        assert_eq!(bin.volumes[0].property::<f64>("volume"), 1.0);
        let _ = bin.bin.set_state(gst::State::Null);
    }

    #[test]
    fn ghost_pad_bin_links_into_muxer_request_pads() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, false) {
            return;
        }
        for (muxer, encoder) in [
            ("mp4mux", AudioEncoder::Aac),
            ("webmmux", AudioEncoder::Opus),
            ("oggmux", AudioEncoder::Opus),
        ] {
            let setup = test_setup(false);
            let bin = build_audio_bin(&setup, encoder, AudioTermination::GhostPad)
                .unwrap_or_else(|err| panic!("{muxer}: {err}"));
            let muxer_element =
                make_element(muxer, "video_muxer").unwrap_or_else(|err| panic!("{muxer}: {err}"));
            let pipeline = gst::Pipeline::new();
            pipeline.add(&bin.bin).unwrap();
            pipeline.add(&muxer_element).unwrap();
            let audio_pad = muxer_element
                .request_pad_simple("audio_%u")
                .expect("muxer audio request pad");
            let ghost = bin.bin.static_pad("src").expect("ghost src pad");
            ghost
                .link(&audio_pad)
                .unwrap_or_else(|err| panic!("{muxer} link failed: {err:?}"));
            let _ = pipeline.set_state(gst::State::Null);
        }
    }

    #[test]
    fn audio_pipeline_feeds_ffmpeg_through_inherited_fd3() {
        use super::super::backend::attach_audio_pipe_as_fd3;

        if skip_if_gst_audio_unavailable(AudioEncoder::Aac, true) {
            return;
        }
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping: ffmpeg not available");
            return;
        }

        let setup = test_setup(false);
        let mut audio =
            ActiveGstAudio::start(&setup, "mp4mux").expect("audio pipeline should start");
        let read_fd = audio.take_read_fd().expect("pipe read fd");

        let out =
            std::env::temp_dir().join(format!("apexshot-audio-pipe-{}.mp4", std::process::id()));
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-nostats"])
            .arg("-f")
            .arg(audio.input_format())
            .arg("-i")
            .arg("pipe:3")
            .arg("-c:a")
            .arg("copy")
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        attach_audio_pipe_as_fd3(&mut cmd, read_fd);

        let mut child = cmd.spawn().expect("ffmpeg should spawn with fd 3");
        std::thread::sleep(Duration::from_millis(800));
        audio.stop();

        let status = wait_for_ffmpeg_or_kill(&mut child, Duration::from_secs(5));
        let stderr = {
            let mut buf = Vec::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = std::io::Read::read_to_end(&mut err, &mut buf);
            }
            buf
        };
        assert!(
            status.success(),
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nw=1",
            ])
            .arg(&out)
            .output()
            .expect("ffprobe should run");
        let streams = String::from_utf8_lossy(&probe.stdout);
        assert!(
            streams.contains("aac"),
            "expected an aac stream in the output, got: {streams}"
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn pause_resume_keeps_live_timestamps_continuous() {
        if skip_if_gst_audio_unavailable(AudioEncoder::Opus, true) {
            return;
        }
        let setup = test_setup(false);
        let bin = build_audio_bin(&setup, AudioEncoder::Opus, AudioTermination::AppSink).unwrap();
        let appsink = bin.appsink.as_ref().unwrap().clone();
        let pipeline = gst::Pipeline::new();
        pipeline.add(&bin.bin).unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();

        let mut first_pts = None;
        let mut last_pts = None;
        let mut record_pts = |sample: &gst::Sample| {
            if let Some(pts) = sample.buffer().and_then(|buffer| buffer.pts()) {
                first_pts.get_or_insert(pts);
                last_pts = Some(pts);
            }
        };

        let phase = Duration::from_millis(500);
        let deadline = Instant::now() + phase;
        while Instant::now() < deadline {
            if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                record_pts(&sample);
            }
        }

        pipeline.set_state(gst::State::Paused).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        pipeline.set_state(gst::State::Playing).unwrap();

        let deadline = Instant::now() + phase;
        while Instant::now() < deadline {
            if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                record_pts(&sample);
            }
        }

        pipeline.send_event(gst::event::Eos::new());
        while !appsink.is_eos() {
            if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                record_pts(&sample);
            }
        }
        let _ = pipeline.set_state(gst::State::Null);

        let (Some(first), Some(last)) = (first_pts, last_pts) else {
            panic!("no timestamps captured");
        };
        let span_ms = (last - first).mseconds();
        let active_ms = 1000u64;
        let pause_gap_ms = 400u64;
        assert!(
            span_ms > active_ms - 350,
            "captured too little audio across both phases (span {span_ms}ms); \
             the timeline assertion below would be vacuous"
        );
        assert!(
            span_ms < active_ms + pause_gap_ms - 150,
            "audio timestamps jumped across the pause (span {span_ms}ms vs active {active_ms}ms); \
             pausing must keep the capture timeline continuous or audio drifts from video"
        );
    }
}
