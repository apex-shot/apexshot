//! Preview playback for the video editor.
//!
//! This GTK bundle ships without GStreamer, so `GtkMediaFile` cannot decode
//! anything. The preview instead asks the same ffmpeg the timeline already
//! uses to emit raw RGBA frames on a pipe: one frame for a paused playhead and
//! a realtime stream while the transport is playing.

use crate::recording::editor::ffmpeg;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::Picture;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Frames are decoded small enough that a raw RGBA frame crosses the pipe
/// quickly, and the `Picture` scales them up into the stage. Either edge is
/// capped, so a portrait recording keeps its height instead of shrinking to a
/// sliver inside a landscape box.
const MAX_FRAME_EDGE: u32 = 1280;
/// While paused, decode again once the playhead has moved a frame's worth.
const STILL_STEP: f64 = 1.0 / 30.0;

struct Frame {
    generation: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

struct Job {
    source: PathBuf,
    seconds: f64,
    width: u32,
    height: u32,
    generation: u64,
    realtime: bool,
}

/// The decode thread owns one ffmpeg at a time. The UI thread keeps the child
/// handle too, so a new request can kill a stream that is blocked on a read
/// instead of waiting for its next frame.
#[derive(Default)]
struct Shared {
    slot: Mutex<Option<Job>>,
    child: Mutex<Option<Child>>,
}

#[derive(Default)]
struct State {
    source: Option<PathBuf>,
    width: u32,
    height: u32,
    playing: bool,
    /// Source time of the last frame requested while paused.
    still_at: Option<f64>,
}

/// The stage's decoder. `Picture` and the presentation tick live on the main
/// thread; decoding happens on a worker thread and only raw pixels cross over.
pub(super) struct Preview {
    picture: Picture,
    state: Rc<RefCell<State>>,
    shared: Arc<Shared>,
    frames: Receiver<Frame>,
    generation: Rc<Cell<u64>>,
}

impl Preview {
    pub fn new(picture: Picture) -> Rc<Self> {
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        {
            let shared = shared.clone();
            std::thread::spawn(move || decode_loop(shared, sender));
        }
        let preview = Rc::new(Self {
            picture,
            state: Rc::new(RefCell::new(State::default())),
            shared,
            frames: receiver,
            generation: Rc::new(Cell::new(0)),
        });
        glib::timeout_add_local(Duration::from_millis(16), {
            let preview = preview.clone();
            move || {
                preview.present();
                glib::ControlFlow::Continue
            }
        });
        preview
    }

    /// Point the decoder at a recording. `width`/`height` are the source
    /// frame's dimensions, read from the poster the timeline already caches.
    pub fn set_document(&self, path: Option<&Path>, width: u32, height: u32) {
        let mut state = self.state.borrow_mut();
        state.source = path.filter(|path| path.is_file()).map(Path::to_path_buf);
        let (width, height) = decode_size(width, height);
        state.width = width;
        state.height = height;
        state.playing = false;
        state.still_at = None;
        drop(state);

        // Anything already decoded belongs to the previous recording.
        self.generation.set(self.generation.get() + 1);
        self.cancel();
    }

    /// Show the frame at a source time. While playing, the stream is only
    /// started once; afterwards it advances on its own and the playhead
    /// follows it. Anything else decodes the exact frame for that time.
    pub fn show_time(&self, seconds: f64, playing: bool) {
        let mut state = self.state.borrow_mut();
        let Some(source) = state.source.clone() else {
            return;
        };
        if playing {
            if !state.playing {
                state.playing = true;
                state.still_at = Some(seconds);
                drop(state);
                self.request(source, seconds, true);
            }
            return;
        }

        // Stopping the transport always repaints the exact frame, even when
        // the playhead has not moved: the stream's last frame is behind it.
        let was_playing = std::mem::replace(&mut state.playing, false);
        let moved = was_playing
            || state
                .still_at
                .is_none_or(|still| (still - seconds).abs() > STILL_STEP);
        if !moved {
            return;
        }
        state.still_at = Some(seconds);
        drop(state);
        self.request(source, seconds, false);
    }

    fn request(&self, source: PathBuf, seconds: f64, realtime: bool) {
        self.generation.set(self.generation.get() + 1);
        let (width, height) = {
            let state = self.state.borrow();
            (state.width, state.height)
        };
        self.cancel();
        *self.shared.slot.lock().unwrap() = Some(Job {
            source,
            seconds,
            width,
            height,
            generation: self.generation.get(),
            realtime,
        });
    }

    fn cancel(&self) {
        if let Some(mut child) = self.shared.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Paint the newest frame for the current request. Frames from a request
    /// that is already stale are dropped instead of flashing on screen.
    fn present(&self) {
        let mut newest = None;
        loop {
            match self.frames.try_recv() {
                Ok(frame) => newest = Some(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let Some(frame) = newest else {
            return;
        };
        if frame.generation != self.generation.get() {
            return;
        }
        let bytes = glib::Bytes::from_owned(frame.pixels);
        let texture = gdk::MemoryTexture::new(
            frame.width as i32,
            frame.height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            frame.width as usize * 4,
        );
        self.picture.set_paintable(Some(&texture));
        self.picture.set_visible(true);
    }
}

fn decode_loop(shared: Arc<Shared>, sender: Sender<Frame>) {
    loop {
        let Some(job) = shared.slot.lock().unwrap().take() else {
            std::thread::sleep(Duration::from_millis(8));
            continue;
        };
        decode(&shared, &sender, job);
    }
}

fn decode(shared: &Shared, sender: &Sender<Frame>, job: Job) {
    let frame_len = job.width as usize * job.height as usize * 4;
    if frame_len == 0 {
        return;
    }
    let mut command = ffmpeg::frame_command(
        &job.source,
        job.seconds,
        job.width,
        job.height,
        job.realtime,
    );
    let Ok(mut child) = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.wait();
        return;
    };
    *shared.child.lock().unwrap() = Some(child);

    let mut buffer = vec![0u8; frame_len];
    loop {
        if stdout.read_exact(&mut buffer).is_err() {
            break;
        }
        let frame = Frame {
            generation: job.generation,
            width: job.width,
            height: job.height,
            pixels: buffer.clone(),
        };
        if sender.send(frame).is_err() {
            break;
        }
        if !job.realtime {
            break;
        }
        // A newer request supersedes this stream.
        if shared.slot.lock().unwrap().is_some() {
            break;
        }
    }

    if let Some(mut child) = shared.child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Decode at the source's aspect ratio, no larger than the preview box, with
/// even dimensions so the RGBA stride is never fractional.
fn decode_size(source_width: u32, source_height: u32) -> (u32, u32) {
    if source_width == 0 || source_height == 0 {
        return (MAX_FRAME_EDGE, MAX_FRAME_EDGE);
    }
    let scale = (MAX_FRAME_EDGE as f64 / source_width as f64)
        .min(MAX_FRAME_EDGE as f64 / source_height as f64)
        .min(1.0);
    let width = ((source_width as f64 * scale).round() as u32).max(2) & !1;
    let height = ((source_height as f64 * scale).round() as u32).max(2) & !1;
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::decode_size;

    #[test]
    fn large_frames_fit_the_preview_box() {
        assert_eq!(decode_size(1920, 1080), (1280, 720));
        assert_eq!(decode_size(3840, 2160), (1280, 720));
        assert_eq!(decode_size(3440, 1440), (1280, 536));
    }

    #[test]
    fn portrait_frames_keep_their_aspect() {
        assert_eq!(decode_size(1080, 1920), (720, 1280));
    }

    #[test]
    fn small_frames_are_not_upscaled_and_stay_even() {
        assert_eq!(decode_size(640, 480), (640, 480));
        assert_eq!(decode_size(641, 481), (640, 480));
        assert_eq!(decode_size(0, 0), (1280, 1280));
    }
}
