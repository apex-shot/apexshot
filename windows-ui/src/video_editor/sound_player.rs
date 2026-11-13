//! The composition's sound while the preview plays.
//!
//! One [`SoundPlayer`] runs per playback: an ffmpeg decode of the composed
//! graph streamed into the default device. [`SoundSync`] keeps a player going
//! while the playhead moves and restarts it on a seek or an edit that changes
//! the sound.

use super::sound::{SoundGraph, SoundPlan};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[cfg(target_os = "windows")]
use std::path::Path;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicU64;

/// The sample rate the graph is decoded at, and the device opens at.
#[cfg(target_os = "windows")]
const SAMPLE_RATE: u32 = 48_000;
/// Samples per buffer: 50 ms, so a queue of six covers the start-up hiccup.
#[cfg(target_os = "windows")]
const BUFFER_FRAMES: usize = 2400;
/// Bytes per frame of the decoded stream: stereo, 16 bits.
#[cfg(target_os = "windows")]
const BLOCK_ALIGN: usize = 4;
/// How many buffers are kept in flight.
#[cfg(target_os = "windows")]
const BUFFER_COUNT: usize = 6;
/// How often a wait for the device checks for a stop.
#[cfg(target_os = "windows")]
const POLL: std::time::Duration = std::time::Duration::from_millis(5);
/// Bytes read ahead while paused: half a second, so a prepared player starts on
/// sound rather than on a decode.
#[cfg(target_os = "windows")]
const PREPARE_BYTES: usize = 48_000 / 2 * BLOCK_ALIGN;
/// How often a prepared player checks for a stop while it waits for go.
#[cfg(target_os = "windows")]
const GO_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// How far the playhead may drift from the sound before it is restarted.
const SYNC_TOLERANCE_SECONDS: f64 = 0.3;

/// Makes every script file of this process unique.
#[cfg(target_os = "windows")]
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The go signal: the instant playback was asked for, and the seconds the
/// playhead has already run past the position the graph starts at.
type Go = Arc<(Mutex<Option<(Instant, f64)>>, Condvar)>;

/// One run of the composition's sound from a composition time.
pub(super) struct SoundPlayer {
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    go: Go,
}

impl SoundPlayer {
    /// Start playing `graph` now. Returns at once; decoding and output run on
    /// a thread.
    pub fn start(graph: SoundGraph) -> Self {
        let player = Self::launch(graph, 0);
        player.go(0.0);
        player
    }

    /// Decode `graph` in advance and hold its first half second until
    /// [`go`](Self::go), so a later play starts on sound.
    pub fn prepare(graph: SoundGraph) -> Self {
        #[cfg(target_os = "windows")]
        let preread = PREPARE_BYTES;
        #[cfg(not(target_os = "windows"))]
        let preread = 0;
        Self::launch(graph, preread)
    }

    /// Let the thread start output. `late` is how many seconds of the stream
    /// the playhead has already run past the graph's position. Only the first
    /// call counts.
    pub fn go(&self, late: f64) {
        let (signal, wake) = &*self.go;
        let Ok(mut slot) = signal.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }
        *slot = Some((Instant::now(), late.max(0.0)));
        drop(slot);
        wake.notify_all();
    }

    /// Spawn the thread. `preread` is how many bytes it reads before it waits
    /// for the go signal.
    fn launch(graph: SoundGraph, preread: usize) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let go: Go = Arc::new((Mutex::new(None), Condvar::new()));
        #[cfg(target_os = "windows")]
        {
            let thread_stop = stop.clone();
            let thread_child = child.clone();
            let thread_go = go.clone();
            std::thread::spawn(move || play(graph, preread, thread_go, thread_stop, thread_child));
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (graph, preread);
        }
        Self { stop, child, go }
    }
}

impl Drop for SoundPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Killing the child ends its stdout, which is what a blocked read
        // waits on; the thread waits for it on its own way out.
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// Write the graph to a script, start ffmpeg on it, and stream what it decodes.
#[cfg(target_os = "windows")]
fn play(
    graph: SoundGraph,
    preread: usize,
    go: Go,
    stop: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<std::process::Child>>>,
) {
    let script = std::env::temp_dir().join(format!(
        "apexshot-preview-sound-{}-{}.txt",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::write(&script, graph.graph.as_bytes()).is_err() {
        return;
    }
    if let Some((mut child, mut stdout)) = spawn(&graph, &script) {
        if stop.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
        } else if let Ok(mut slot) = child_slot.lock() {
            *slot = Some(child);
            drop(slot);
            output(&mut stdout, &go, preread, &stop);
        } else {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    if let Ok(mut slot) = child_slot.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    let _ = std::fs::remove_file(&script);
}

/// ffmpeg decoding the script's graph into raw stereo samples on its stdout.
#[cfg(target_os = "windows")]
fn spawn(
    graph: &SoundGraph,
    script: &Path,
) -> Option<(std::process::Child, std::process::ChildStdout)> {
    use std::process::Stdio;
    let mut command =
        crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffmpeg_executable());
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
    command
        .arg(super::sound::graph_file_flag_for_this_ffmpeg())
        .arg(script)
        .args([
            "-map", "[a]", "-f", "s16le", "-ac", "2", "-ar", "48000", "pipe:1",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    Some((child, stdout))
}

/// Open the default device and queue the stream into it until it ends or the
/// player is stopped.
#[cfg(target_os = "windows")]
fn output(stdout: &mut std::process::ChildStdout, go: &Go, preread: usize, stop: &AtomicBool) {
    use std::io::Read;
    let format = WaveFormatEx {
        format_tag: WAVE_FORMAT_PCM,
        channels: 2,
        samples_per_sec: SAMPLE_RATE,
        avg_bytes_per_sec: SAMPLE_RATE * BLOCK_ALIGN as u32,
        block_align: BLOCK_ALIGN as u16,
        bits_per_sample: 16,
        cb_size: 0,
    };
    let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
    if unsafe { waveOutOpen(&mut handle, WAVE_MAPPER, &format, 0, 0, CALLBACK_NULL) } != 0 {
        return;
    }
    let mut buffers: Vec<Buffer> = (0..BUFFER_COUNT).map(|_| Buffer::new()).collect();
    let mut scratch = vec![0u8; BUFFER_FRAMES * BLOCK_ALIGN];
    let mut ended = false;
    // The stream is read ahead of the go signal, so a prepared player holds
    // its first half second by the time playback starts.
    let mut pending: Vec<u8> = Vec::new();
    while !ended && !stop.load(Ordering::SeqCst) {
        match stdout.read(&mut scratch) {
            Ok(0) => {
                ended = true;
                break;
            }
            Ok(read) => {
                pending.extend_from_slice(&scratch[..read]);
                if pending.len() >= preread {
                    break;
                }
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => {
                ended = true;
                break;
            }
        }
    }
    let mut started = None;
    if !ended {
        started = wait_for_go(go, stop);
        ended = started.is_none();
    }
    // The picture kept moving while ffmpeg started, so the stream has to skip
    // as much sound as the playhead has already run. Measured from the go
    // signal, so a prepared player skips only what the playhead ran past it.
    if let Some((instant, late)) = started {
        let mut skip = ((late + instant.elapsed().as_secs_f64()) * f64::from(SAMPLE_RATE)) as usize
            * BLOCK_ALIGN;
        let discard = skip.min(pending.len());
        pending.drain(..discard);
        skip -= discard;
        while skip > 0 && !stop.load(Ordering::SeqCst) {
            let want = skip.min(scratch.len());
            match stdout.read(&mut scratch[..want]) {
                Ok(0) => {
                    ended = true;
                    break;
                }
                Ok(read) => skip -= read,
                Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    ended = true;
                    break;
                }
            }
        }
    }
    let mut next = 0usize;
    while !ended && !stop.load(Ordering::SeqCst) {
        let buffer = &mut buffers[next];
        while buffer.written && !buffer.done() {
            if stop.load(Ordering::SeqCst) {
                ended = true;
                break;
            }
            std::thread::sleep(POLL);
        }
        if ended {
            break;
        }
        let mut filled = 0usize;
        // Bytes read ahead, and whatever the skip left of them, fill the
        // buffers before the stream is read again.
        if !pending.is_empty() {
            let take = pending.len().min(buffer.samples.len());
            buffer.samples[..take].copy_from_slice(&pending[..take]);
            pending.drain(..take);
            filled = take;
        }
        while filled < buffer.samples.len() {
            match stdout.read(&mut buffer.samples[filled..]) {
                Ok(0) => {
                    ended = true;
                    break;
                }
                Ok(read) => filled += read,
                Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    ended = true;
                    break;
                }
            }
        }
        // A device plays whole frames only, so a short final read is cut to
        // the last complete one.
        let filled = filled - filled % BLOCK_ALIGN;
        if filled == 0 {
            break;
        }
        buffer.length = filled as u32;
        if !buffer.queue(handle) {
            break;
        }
        next = (next + 1) % BUFFER_COUNT;
    }
    while !stop.load(Ordering::SeqCst)
        && buffers
            .iter()
            .any(|buffer| buffer.written && !buffer.done())
    {
        std::thread::sleep(POLL);
    }
    unsafe {
        waveOutReset(handle);
        for buffer in &mut buffers {
            if buffer.prepared {
                waveOutUnprepareHeader(handle, &mut *buffer.header, HEADER_SIZE);
            }
        }
        waveOutClose(handle);
    }
}

/// Wait for the go signal, checking for a stop while it does. None when the
/// player was stopped before it was told to go.
#[cfg(target_os = "windows")]
fn wait_for_go(go: &Go, stop: &AtomicBool) -> Option<(Instant, f64)> {
    let (signal, wake) = &**go;
    let mut slot = signal.lock().ok()?;
    while slot.is_none() {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        slot = wake.wait_timeout(slot, GO_POLL).ok()?.0;
    }
    slot.take()
}

/// The size of the header struct the device API takes.
#[cfg(target_os = "windows")]
const HEADER_SIZE: u32 = std::mem::size_of::<WaveHdr>() as u32;

/// One buffer and the header the device reads it through. The header is boxed
/// and the samples live on the heap, so neither address moves while the device
/// owns them.
#[cfg(target_os = "windows")]
struct Buffer {
    header: Box<WaveHdr>,
    samples: Vec<u8>,
    /// Bytes of `samples` the last read filled.
    length: u32,
    prepared: bool,
    written: bool,
}

#[cfg(target_os = "windows")]
impl Buffer {
    fn new() -> Self {
        let mut samples = vec![0u8; BUFFER_FRAMES * BLOCK_ALIGN];
        let header = Box::new(WaveHdr {
            data: samples.as_mut_ptr(),
            buffer_length: 0,
            bytes_recorded: 0,
            user: 0,
            flags: 0,
            loops: 0,
            next: std::ptr::null_mut(),
            reserved: 0,
        });
        Self {
            header,
            samples,
            length: 0,
            prepared: false,
            written: false,
        }
    }

    /// Whether the device has finished with the last write.
    fn done(&self) -> bool {
        // The driver writes the flag from its own thread while this waits.
        unsafe { std::ptr::read_volatile(std::ptr::addr_of!(self.header.flags)) & WHDR_DONE != 0 }
    }

    /// Hand `length` bytes to the device, preparing the header first.
    fn queue(&mut self, handle: *mut std::ffi::c_void) -> bool {
        unsafe {
            if self.prepared {
                waveOutUnprepareHeader(handle, &mut *self.header, HEADER_SIZE);
                self.prepared = false;
            }
            self.header.data = self.samples.as_mut_ptr();
            self.header.buffer_length = self.length;
            self.header.bytes_recorded = 0;
            std::ptr::write_volatile(std::ptr::addr_of_mut!(self.header.flags), 0);
            if waveOutPrepareHeader(handle, &mut *self.header, HEADER_SIZE) != 0 {
                return false;
            }
            self.prepared = true;
            if waveOutWrite(handle, &mut *self.header, HEADER_SIZE) != 0 {
                return false;
            }
        }
        self.written = true;
        true
    }
}

#[cfg(target_os = "windows")]
#[repr(C, packed(1))]
struct WaveFormatEx {
    format_tag: u16,
    channels: u16,
    samples_per_sec: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
    cb_size: u16,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WaveHdr {
    data: *mut u8,
    buffer_length: u32,
    bytes_recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHdr,
    reserved: usize,
}

#[cfg(target_os = "windows")]
const WAVE_MAPPER: u32 = u32::MAX;
#[cfg(target_os = "windows")]
const CALLBACK_NULL: u32 = 0;
#[cfg(target_os = "windows")]
const WHDR_DONE: u32 = 1;
#[cfg(target_os = "windows")]
const WAVE_FORMAT_PCM: u16 = 1;

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn waveOutOpen(
        handle: *mut *mut std::ffi::c_void,
        device: u32,
        format: *const WaveFormatEx,
        callback: usize,
        instance: usize,
        flags: u32,
    ) -> u32;
    fn waveOutPrepareHeader(handle: *mut std::ffi::c_void, header: *mut WaveHdr, size: u32) -> u32;
    fn waveOutUnprepareHeader(
        handle: *mut std::ffi::c_void,
        header: *mut WaveHdr,
        size: u32,
    ) -> u32;
    fn waveOutWrite(handle: *mut std::ffi::c_void, header: *mut WaveHdr, size: u32) -> u32;
    fn waveOutReset(handle: *mut std::ffi::c_void) -> u32;
    fn waveOutClose(handle: *mut std::ffi::c_void) -> u32;
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

/// Whether a prepared player still matches the playhead: the same sound, and a
/// position inside the half second it read ahead.
fn primed_matches(primed_key: &str, primed_time: f64, key: &str, time: f64) -> bool {
    primed_key == key && (0.0..0.5).contains(&(time - primed_time))
}

/// Keeps one player running while the preview plays, restarting it on a seek or
/// on an edit that changes the sound, and prepares one for the position the
/// preview is paused on.
pub(super) struct SoundSync {
    player: RefCell<Option<SoundPlayer>>,
    key: RefCell<Option<String>>,
    started: Cell<Option<(Instant, f64)>>,
    click: RefCell<Option<PathBuf>>,
    primed: RefCell<Option<(SoundPlayer, String, f64)>>,
    prime_timer: RefCell<Option<gtk4::glib::SourceId>>,
    wanted: RefCell<Option<(String, f64)>>,
}

impl SoundSync {
    pub fn new() -> Self {
        Self {
            player: RefCell::new(None),
            key: RefCell::new(None),
            started: Cell::new(None),
            click: RefCell::new(None),
            primed: RefCell::new(None),
            prime_timer: RefCell::new(None),
            wanted: RefCell::new(None),
        }
    }

    /// Called on every preview frame. `key` describes everything the sound
    /// depends on; `plan` builds the plan only when a (re)start or a
    /// preparation is needed.
    pub fn update(
        self: &Rc<Self>,
        playing: bool,
        time: f64,
        key: String,
        plan: impl FnOnce() -> Option<SoundPlan> + 'static,
    ) {
        if !playing {
            // Paused: the player stops, and one is prepared for the position
            // the preview has settled on.
            self.player.borrow_mut().take();
            *self.key.borrow_mut() = None;
            self.started.set(None);
            let primed = self.primed.borrow();
            let settled = primed.as_ref().is_some_and(|(_, primed_key, primed_time)| {
                primed_key == &key && (primed_time - time).abs() < 1e-6
            });
            drop(primed);
            if settled {
                return;
            }
            let wanted = self.wanted.borrow();
            let queued = wanted.as_ref().is_some_and(|(wanted_key, wanted_time)| {
                wanted_key == &key && *wanted_time == time
            });
            drop(wanted);
            if queued {
                return;
            }
            *self.primed.borrow_mut() = None;
            *self.wanted.borrow_mut() = Some((key.clone(), time));
            self.cancel_prime();
            self.schedule_prime(key, time, Box::new(plan));
            return;
        }
        self.cancel_prime();
        // The preparation the timer was going to run is gone with it.
        *self.wanted.borrow_mut() = None;
        let running = self.player.borrow().is_some() || self.started.get().is_some();
        if !running {
            if let Some((player, primed_key, primed_time)) = self.primed.borrow_mut().take() {
                if primed_matches(&primed_key, primed_time, &key, time) {
                    player.go(time - primed_time);
                    *self.player.borrow_mut() = Some(player);
                    *self.key.borrow_mut() = Some(key);
                    self.started.set(Some((Instant::now(), time)));
                    return;
                }
            }
        }
        *self.primed.borrow_mut() = None;
        let same_key = self.key.borrow().as_deref() == Some(key.as_str());
        let started = self
            .started
            .get()
            .map(|(instant, at)| (instant.elapsed().as_secs_f64(), at));
        if !needs_restart(running, same_key, started, time) {
            return;
        }
        self.player.borrow_mut().take();
        let graph = plan().and_then(|plan| {
            let click = self.click_path(&plan);
            plan.graph_from(time, click.as_deref(), 0)
        });
        *self.player.borrow_mut() = graph.map(SoundPlayer::start);
        *self.key.borrow_mut() = Some(key);
        self.started.set(Some((Instant::now(), time)));
    }

    /// Prepare a player for `key` at `time` once the playhead has settled on
    /// it for longer than a scrub takes.
    fn schedule_prime(
        self: &Rc<Self>,
        key: String,
        time: f64,
        plan: Box<dyn FnOnce() -> Option<SoundPlan>>,
    ) {
        let sync = Rc::downgrade(self);
        let source =
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                let Some(sync) = sync.upgrade() else {
                    return;
                };
                // A source that has fired is out of the loop already, so its
                // id is taken out before anything could remove it again.
                sync.prime_timer.borrow_mut().take();
                let wanted = sync.wanted.borrow();
                let current = wanted.as_ref().is_some_and(|(wanted_key, wanted_time)| {
                    wanted_key == &key && *wanted_time == time
                });
                drop(wanted);
                if !current {
                    return;
                }
                *sync.wanted.borrow_mut() = None;
                let graph = plan().and_then(|plan| {
                    let click = sync.click_path(&plan);
                    plan.graph_from(time, click.as_deref(), 0)
                });
                if let Some(graph) = graph {
                    *sync.primed.borrow_mut() = Some((SoundPlayer::prepare(graph), key, time));
                }
            });
        *self.prime_timer.borrow_mut() = Some(source);
    }

    /// Drop a scheduled preparation, if any.
    fn cancel_prime(&self) {
        if let Some(source) = self.prime_timer.borrow_mut().take() {
            source.remove();
        }
    }

    /// Stop any sound at once, prepared or not.
    pub fn stop(&self) {
        self.cancel_prime();
        self.player.borrow_mut().take();
        *self.primed.borrow_mut() = None;
        *self.key.borrow_mut() = None;
        self.started.set(None);
        *self.wanted.borrow_mut() = None;
    }

    /// The click-sound file, written once the first plan carries cues.
    fn click_path(&self, plan: &SoundPlan) -> Option<PathBuf> {
        if plan.cues.is_empty() {
            return None;
        }
        if let Some(path) = self.click.borrow().as_ref() {
            return Some(path.clone());
        }
        let path =
            std::env::temp_dir().join(format!("apexshot-preview-click-{}.wav", std::process::id()));
        std::fs::write(&path, super::click_sound::WAV).ok()?;
        *self.click.borrow_mut() = Some(path.clone());
        Some(path)
    }
}

impl Drop for SoundSync {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_restart, primed_matches};

    #[test]
    fn a_prepared_player_matches_its_own_position() {
        assert!(primed_matches("a", 2.0, "a", 2.0));
    }

    #[test]
    fn a_prepared_player_covers_the_half_second_it_read() {
        assert!(primed_matches("a", 2.0, "a", 2.1));
    }

    #[test]
    fn a_playhead_off_the_prepared_player_does_not_match() {
        assert!(!primed_matches("a", 2.0, "a", 1.9));
        assert!(!primed_matches("a", 2.0, "a", 2.5));
    }

    #[test]
    fn a_changed_sound_does_not_match_a_prepared_player() {
        assert!(!primed_matches("a", 2.0, "b", 2.0));
    }

    #[test]
    fn a_player_that_is_not_running_starts() {
        assert!(needs_restart(false, true, Some((0.0, 0.0)), 0.0));
        assert!(needs_restart(false, false, None, 5.0));
    }

    #[test]
    fn a_changed_sound_restarts_the_player() {
        assert!(needs_restart(true, false, Some((0.5, 2.0)), 2.5));
    }

    #[test]
    fn a_playhead_on_the_sound_leaves_it_running() {
        assert!(!needs_restart(true, true, Some((0.5, 2.0)), 2.5));
        assert!(!needs_restart(true, true, Some((0.2, 10.0)), 10.2));
    }

    #[test]
    fn a_jump_of_a_second_restarts_the_player() {
        assert!(needs_restart(true, true, Some((0.5, 2.0)), 3.5));
    }

    #[test]
    fn a_silent_plan_is_not_rebuilt_on_every_frame() {
        assert!(!needs_restart(true, true, Some((1.0, 5.0)), 6.0));
    }
}
