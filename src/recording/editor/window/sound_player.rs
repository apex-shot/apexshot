//! The composition's sound while the preview plays.
//!
//! One [`SoundPlayer`] decodes the sound graph with ffmpeg and streams the
//! samples into a GStreamer pipeline of its own, so the editor's sound is a
//! separate audio-server stream from the video's — the video stream is kept
//! muted and this one carries every clip's own mute, the microphone mode, the
//! music and the clicks. [`SoundSync`] keeps a player going while the playhead
//! moves and restarts it on a seek or an edit that changes the sound.

use crate::recording::editor::sound::{SoundGraph, SoundPlan, CLICK_WAV};
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use gst::prelude::*;

/// The rate the graph is decoded at, and the pipeline plays at.
const SAMPLE_RATE: u32 = 48_000;
/// The channels the decoded stream carries.
const CHANNELS: u32 = 2;
/// Bytes per sample of the decoded stream: interleaved 16-bit.
const SAMPLE_BYTES: usize = 2;
/// Bytes handed to the pipeline at once: 20 ms, small enough that a stop does
/// not wait on a long decode.
const CHUNK_BYTES: usize = SAMPLE_BYTES * CHANNELS as usize * (SAMPLE_RATE as usize / 50);

/// How far the playhead may drift from the sound before it is restarted.
const SYNC_TOLERANCE_SECONDS: f64 = 0.3;

/// The stream name the editor's sound runs under. It has to differ from the
/// video's: the audio server remembers a mute per application name, and the
/// editor's video stream is muted on purpose.
const STREAM_NAME: &str = "ApexShot editor sound";

/// One run of the composition's sound from a composition time.
pub(super) struct SoundPlayer {
    pipeline: gst::Pipeline,
    stop: Arc<AtomicBool>,
    child: Option<Child>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl SoundPlayer {
    /// Start playing `graph` now. None when the decoder or the pipeline could
    /// not be started, which leaves the preview silent rather than failing.
    pub fn start(graph: SoundGraph) -> Option<Self> {
        let mut child = spawn_decoder(&graph)?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let (pipeline, appsrc) = match build_pipeline() {
            Ok(built) => built,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        };
        if pipeline.set_state(gst::State::Playing).is_err() {
            let _ = pipeline.set_state(gst::State::Null);
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("apexshot-preview-sound".into())
                .spawn(move || feed(stdout, appsrc, stop))
                .ok()
        };
        Some(Self {
            pipeline,
            stop,
            child: Some(child),
            reader,
        })
    }
}

impl Drop for SoundPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Null first: it ends the flushing state the sink buffers are in, so a
        // reader blocked in a push returns instead of waiting for samples.
        let _ = self.pipeline.set_state(gst::State::Null);
        // Then the decoder: killing it closes the pipe the reader waits on.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Whether the preview can play the composition's sound: ffmpeg on the path
/// and the GStreamer pieces the pipeline is built from. Read once.
fn playback_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        if crate::recording::gst_audio::ensure_gst_initialized().is_err() {
            return false;
        }
        for name in ["appsrc", "audioconvert", "audioresample"] {
            if gst::ElementFactory::find(name).is_none() {
                return false;
            }
        }
        if gst::ElementFactory::find("pulsesink").is_none()
            && gst::ElementFactory::find("autoaudiosink").is_none()
        {
            return false;
        }
        Command::new("ffmpeg")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Start the decoder for a graph, streaming raw samples on stdout.
fn spawn_decoder(graph: &SoundGraph) -> Option<Child> {
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    for input in &graph.inputs {
        if input.looped {
            command.args(["-stream_loop", "-1"]);
        }
        if input.seek > 0.0 {
            command.arg("-ss").arg(format!("{:.6}", input.seek));
        }
        command.arg("-i").arg(&input.path);
    }
    command.arg("-filter_complex").arg(&graph.graph);
    command.arg("-map").arg("[a]");
    command.arg("-f").arg("s16le");
    command.arg("-ac").arg(CHANNELS.to_string());
    command.arg("-ar").arg(SAMPLE_RATE.to_string());
    command.arg("pipe:1");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Build the playback pipeline: the samples arrive on an `appsrc` and leave
/// through the sound server's sink.
fn build_pipeline() -> Result<(gst::Pipeline, gst_app::AppSrc), String> {
    crate::recording::gst_audio::ensure_gst_initialized()?;
    let caps = gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("channels", CHANNELS as i32)
        .field("rate", SAMPLE_RATE as i32)
        .field("layout", "interleaved")
        .build();
    let appsrc = gst_app::AppSrc::builder()
        .name("apexshot_sound_src")
        .caps(&caps)
        .format(gst::Format::Time)
        .stream_type(gst_app::AppStreamType::Stream)
        .build();
    let convert = make("audioconvert")?;
    let resample = make("audioresample")?;
    let sink = make_sink()?;
    let source: gst::Element = appsrc.clone().upcast();
    let pipeline = gst::Pipeline::new();
    pipeline
        .add_many([&source, &convert, &resample, &sink])
        .map_err(|error| format!("could not build the sound pipeline: {error}"))?;
    gst::Element::link_many([&source, &convert, &resample, &sink])
        .map_err(|error| format!("could not link the sound pipeline: {error}"))?;
    Ok((pipeline, appsrc))
}

fn make(name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|error| format!("missing GStreamer element '{name}': {error}"))
}

/// The sound-server sink, marked with the editor's own stream name so it is
/// not confused with the muted video stream.
fn make_sink() -> Result<gst::Element, String> {
    if gst::ElementFactory::find("pulsesink").is_some() {
        let sink = make("pulsesink")?;
        sink.set_property("stream-properties", stream_properties());
        return Ok(sink);
    }
    make("autoaudiosink")
}

fn stream_properties() -> gst::Structure {
    gst::Structure::builder("props")
        .field("application.name", STREAM_NAME)
        .field("media.role", "video")
        .build()
}

/// Read the decoded samples and push them on, stamping each buffer so the sink
/// plays them on a continuous clock instead of as fast as ffmpeg decodes.
fn feed(mut stdout: impl Read, appsrc: gst_app::AppSrc, stop: Arc<AtomicBool>) {
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut frames: u64 = 0;
    let frame_bytes = SAMPLE_BYTES * CHANNELS as usize;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let read = match stdout.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let count = read / frame_bytes;
        if count == 0 {
            continue;
        }
        let mut samples = gst::Buffer::from_slice(buffer[..count * frame_bytes].to_vec());
        {
            let samples = samples.get_mut().expect("a fresh buffer is writable");
            samples.set_pts(gst::ClockTime::from_nseconds(
                frames * 1_000_000_000 / u64::from(SAMPLE_RATE),
            ));
            samples.set_duration(gst::ClockTime::from_nseconds(
                count as u64 * 1_000_000_000 / u64::from(SAMPLE_RATE),
            ));
        }
        frames += count as u64;
        if appsrc.push_buffer(samples).is_err() {
            break;
        }
    }
    let _ = appsrc.end_of_stream();
}

/// Whether the running player should be replaced: none is running, the sound
/// changed, or the playhead has left the position the sound predicts.
fn needs_restart(running: bool, same_key: bool, started: Option<(f64, f64)>, time: f64) -> bool {
    if !running || !same_key {
        return true;
    }
    let Some((elapsed, at)) = started else {
        return true;
    };
    (time - (at + elapsed)).abs() > SYNC_TOLERANCE_SECONDS
}

/// Keeps one player running while the preview plays, restarting it on a seek or
/// on an edit that changes the sound.
pub(super) struct SoundSync {
    player: RefCell<Option<SoundPlayer>>,
    key: RefCell<Option<String>>,
    started: Cell<Option<(Instant, f64)>>,
    /// The key whose player could not be started, so a broken sink is not
    /// retried on every frame.
    failed: RefCell<Option<String>>,
    click: RefCell<Option<PathBuf>>,
}

impl SoundSync {
    pub fn new() -> Self {
        Self {
            player: RefCell::new(None),
            key: RefCell::new(None),
            started: Cell::new(None),
            failed: RefCell::new(None),
            click: RefCell::new(None),
        }
    }

    /// Whether the preview plays the composition's sound at all. When it does
    /// not, the video's own stream keeps its sound instead.
    pub fn available(&self) -> bool {
        playback_available()
    }

    /// Called on every preview frame. `key` describes everything the sound
    /// depends on; the plan is built only when a (re)start is needed.
    pub fn update(&self, playing: bool, time: f64, key: String, plan: impl FnOnce() -> SoundPlan) {
        if !playing {
            self.stop();
            return;
        }
        if self.failed.borrow().as_deref() == Some(key.as_str()) {
            return;
        }
        let running = self.player.borrow().is_some();
        let same_key = self.key.borrow().as_deref() == Some(key.as_str());
        let started = self
            .started
            .get()
            .map(|(instant, at)| (instant.elapsed().as_secs_f64(), at));
        if !needs_restart(running, same_key, started, time) {
            return;
        }
        self.player.borrow_mut().take();
        *self.key.borrow_mut() = Some(key.clone());
        self.started.set(Some((Instant::now(), time)));
        let plan = plan();
        let click = self.click_path(&plan);
        if let Some(graph) = plan.graph_from(time, click.as_deref(), 0) {
            *self.player.borrow_mut() = SoundPlayer::start(graph);
        }
        if self.player.borrow().is_none() {
            *self.failed.borrow_mut() = Some(key);
        }
    }

    /// Stop any sound at once.
    pub fn stop(&self) {
        self.player.borrow_mut().take();
        *self.key.borrow_mut() = None;
        self.started.set(None);
    }

    /// The click-sound file, written once the first plan carries cues.
    fn click_path(&self, plan: &SoundPlan) -> Option<PathBuf> {
        if !plan.has_cues() {
            return None;
        }
        if let Some(path) = self.click.borrow().as_ref() {
            return Some(path.clone());
        }
        let path =
            std::env::temp_dir().join(format!("apexshot-preview-click-{}.wav", std::process::id()));
        std::fs::write(&path, CLICK_WAV).ok()?;
        *self.click.borrow_mut() = Some(path.clone());
        Some(path)
    }
}

impl Default for SoundSync {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SoundSync {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_restart, SYNC_TOLERANCE_SECONDS};

    #[test]
    fn a_running_player_keeps_going_while_the_playhead_agrees() {
        assert!(!needs_restart(true, true, Some((1.0, 2.0)), 3.0));
    }

    #[test]
    fn a_playhead_off_the_sound_restarts_it() {
        assert!(needs_restart(
            true,
            true,
            Some((1.0, 2.0)),
            3.0 + SYNC_TOLERANCE_SECONDS + 0.01
        ));
        assert!(needs_restart(true, true, Some((1.0, 2.0)), 2.0));
    }

    #[test]
    fn a_changed_sound_or_a_stopped_player_restarts_it() {
        assert!(needs_restart(false, true, Some((1.0, 2.0)), 3.0));
        assert!(needs_restart(true, false, Some((1.0, 2.0)), 3.0));
        assert!(needs_restart(true, true, None, 3.0));
    }

    #[test]
    fn the_stream_name_differs_from_the_video_stream() {
        let properties = super::stream_properties();
        assert_eq!(
            properties.get::<&str>("application.name").unwrap(),
            super::STREAM_NAME
        );
        assert_ne!(super::STREAM_NAME, "apexshot");
    }

    #[test]
    fn a_missing_click_plan_needs_no_file() {
        let sync = super::SoundSync::new();
        let plan = crate::recording::editor::sound::SoundPlan::from_state(
            &crate::recording::editor::model::VideoEditState::new(
                crate::recording::editor::model::VideoMetadata {
                    path: std::env::temp_dir().join("apexshot-sound-sync.mp4"),
                    duration_seconds: 2.0,
                    width: 640,
                    height: 480,
                    file_size_bytes: 1,
                    has_audio: false,
                    frame_rate: 30.0,
                },
            ),
        );
        assert!(
            sync.click_path(&plan).is_none(),
            "a plan without cues writes nothing"
        );
    }
}
