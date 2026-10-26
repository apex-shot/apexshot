//! Preview playback for the video editor.
//!
//! This GTK bundle ships without GStreamer, so `GtkMediaFile` cannot decode
//! anything. The preview instead asks the same ffmpeg the timeline already
//! uses to emit raw RGBA frames on a pipe: one frame for a paused playhead and
//! a realtime stream while the transport is playing.

use crate::recording::editor::ffmpeg;
use crate::recording::editor::ripple_preview::PreviewRipple;
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

/// The raw frame last shown while a warp is running, kept so a changing ripple
/// can re-warp it without decoding again. Holding it is only worth the memory
/// during the ripple's visible lifetime.
struct RawFrame {
    generation: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// The stage's decoder. `Picture` and the presentation tick live on the main
/// thread; decoding happens on a worker thread and only raw pixels cross over.
pub(super) struct Preview {
    picture: Picture,
    state: Rc<RefCell<State>>,
    shared: Arc<Shared>,
    frames: Receiver<Frame>,
    generation: Rc<Cell<u64>>,
    ripple: RefCell<Option<PreviewRipple>>,
    ripple_dirty: Cell<bool>,
    raw: RefCell<Option<RawFrame>>,
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
            ripple: RefCell::new(None),
            ripple_dirty: Cell::new(false),
            raw: RefCell::new(None),
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
        *self.ripple.borrow_mut() = None;
        self.ripple_dirty.set(false);
        *self.raw.borrow_mut() = None;
    }

    /// Set the ripple the current frame should warp. A change while the
    /// transport is paused re-works the frame already on screen, so scrubbing
    /// into a click's lifetime shows the wave without waiting for a decode.
    pub fn set_ripple(&self, ripple: Option<PreviewRipple>) {
        let mut slot = self.ripple.borrow_mut();
        if *slot == ripple {
            return;
        }
        *slot = ripple;
        self.ripple_dirty.set(true);
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
        let newest = self.newest_frame();
        let ripple = self.ripple.borrow().clone();
        let dirty = self.ripple_dirty.replace(false);

        if let Some(frame) = newest {
            match &ripple {
                Some(ripple) => self.present_warped(ripple, frame),
                None => {
                    *self.raw.borrow_mut() = None;
                    self.show(frame.pixels, frame.width, frame.height);
                }
            }
            return;
        }
        // While the stream is running, every new frame warps with the ages of
        // its own arrival; re-warping the same still between them only repeats
        // that work. A paused playhead does need this path: its frame is static
        // and a scrub or effect change has to reach the picture without a new
        // decode.
        if !dirty || self.state.borrow().playing {
            return;
        }
        match &ripple {
            Some(ripple) => {
                let warped = {
                    let raw = self.raw.borrow();
                    raw.as_ref()
                        .filter(|raw| raw.generation == self.generation.get())
                        .map(|raw| {
                            (
                                warp_frame(ripple, &raw.pixels, (raw.width, raw.height)),
                                raw.width,
                                raw.height,
                            )
                        })
                };
                if let Some((pixels, width, height)) = warped.filter(|warped| !warped.0.is_empty())
                {
                    self.show(pixels, width, height);
                }
            }
            None => {
                if let Some(raw) = self.raw.borrow_mut().take() {
                    self.show(raw.pixels, raw.width, raw.height);
                }
            }
        }
    }

    fn present_warped(&self, ripple: &PreviewRipple, frame: Frame) {
        let warped = warp_frame(ripple, &frame.pixels, (frame.width, frame.height));
        if warped.is_empty() {
            *self.raw.borrow_mut() = None;
            self.show(frame.pixels, frame.width, frame.height);
            return;
        }
        self.show(warped, frame.width, frame.height);
        *self.raw.borrow_mut() = Some(RawFrame {
            generation: frame.generation,
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels,
        });
    }

    fn newest_frame(&self) -> Option<Frame> {
        let mut newest = None;
        loop {
            match self.frames.try_recv() {
                Ok(frame) => newest = Some(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        newest.filter(|frame| frame.generation == self.generation.get())
    }

    fn show(&self, pixels: Vec<u8>, width: u32, height: u32) {
        let bytes = glib::Bytes::from_owned(pixels);
        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            width as usize * 4,
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

/// Warp one decoded frame. The texture holds the whole encoded frame, so the
/// ripple's view is the full source: the picture's camera transform is applied
/// afterwards and magnifies the wave together with the footage, which keeps it
/// over the source pixel the click was on at any zoom.
pub(super) fn warp_frame(ripple: &PreviewRipple, pixels: &[u8], size: (u32, u32)) -> Vec<u8> {
    ripple.warp_pixels(
        pixels,
        size,
        (0.0, 0.0, f64::from(size.0), f64::from(size.1)),
        size,
    )
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
    use super::{decode_size, warp_frame};
    use crate::recording::editor::click_effect::{ripple_pull_px, RIPPLE_PULL_SIZE_01};
    use crate::recording::editor::ripple_preview::{PreviewRipple, PreviewRippleClick};
    use crate::video_editor::zoom;

    fn gradient(size: (u32, u32)) -> Vec<u8> {
        let (width, height) = size;
        let x_scale = 255.0 / (width - 1).max(1) as f64;
        let y_scale = 255.0 / (height - 1).max(1) as f64;
        (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    [
                        (x as f64 * x_scale).round() as u8,
                        (y as f64 * y_scale).round() as u8,
                        0,
                        255,
                    ]
                })
            })
            .collect()
    }

    fn frame_ripple(source: (f64, f64), clicks: &[(f64, f64, f64)]) -> PreviewRipple {
        PreviewRipple {
            view: (0.0, 0.0, source.0, source.1),
            source_width: source.0,
            source_height: source.1,
            clicks: clicks
                .iter()
                .map(|&(x, y, age_ms)| PreviewRippleClick { x, y, age_ms })
                .collect(),
        }
    }

    fn pixel_at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * width + x) * 4) as usize;
        pixels[offset..offset + 4].try_into().unwrap()
    }

    /// The source pixel a texture pixel shows, read out of the frame's
    /// coordinate-encoding gradient. Bilinear sampling of a linear gradient
    /// is exact apart from the 8-bit rounding.
    fn texture_source(
        pixels: &[u8],
        image: (u32, u32),
        source: (f64, f64),
        point: (u32, u32),
    ) -> (f64, f64) {
        let [red, green, _, _] = pixel_at(pixels, image.0, point.0, point.1);
        (
            (f64::from(red) / 255.0 * f64::from(image.0 - 1) + 0.5) * source.0 / f64::from(image.0),
            (f64::from(green) / 255.0 * f64::from(image.1 - 1) + 0.5) * source.1
                / f64::from(image.1),
        )
    }

    #[test]
    fn a_click_displaces_the_texture_pixel_over_its_source_point() {
        let source = (1920.0, 1080.0);
        let image = (960, 540);
        let pixels = gradient(image);
        let click = (1000.0, 500.0, 400.0);
        let warped = warp_frame(&frame_ripple(source, &[click]), &pixels, image);
        assert_ne!(warped, pixels, "the wave must move footage");

        let point = (500, 250);
        let sampled = texture_source(&warped, image, source, point);
        let source_point = (
            (f64::from(point.0) + 0.5) * source.0 / f64::from(image.0),
            (f64::from(point.1) + 0.5) * source.1 / f64::from(image.1),
        );
        let pull = ripple_pull_px(
            source_point.0,
            source_point.1,
            click.0,
            click.1,
            click.2,
            RIPPLE_PULL_SIZE_01,
            source.0,
        );
        assert!(
            (sampled.0 - (source_point.0 - pull.0)).abs() < 5.0,
            "sampled {sampled:?} from {source_point:?} with pull {pull:?}"
        );
        assert!(
            (sampled.1 - (source_point.1 - pull.1)).abs() < 5.0,
            "sampled {sampled:?} from {source_point:?} with pull {pull:?}"
        );
        assert_eq!(
            pixel_at(&warped, image.0, 10, 10),
            pixel_at(&pixels, image.0, 10, 10),
            "footage far from the wave is untouched"
        );
    }

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

    #[test]
    fn a_smaller_decode_keeps_the_wave_on_the_same_source_pixel() {
        let source = (1920.0, 1080.0);
        let ripple = frame_ripple(source, &[(1000.0, 500.0, 400.0)]);
        let large = warp_frame(&ripple, &gradient((960, 540)), (960, 540));
        let small = warp_frame(&ripple, &gradient((480, 270)), (480, 270));
        let from_large = texture_source(&large, (960, 540), source, (500, 250));
        let from_small = texture_source(&small, (480, 270), source, (250, 125));
        assert!(
            (from_large.0 - from_small.0).abs() < 8.0 && (from_large.1 - from_small.1).abs() < 8.0,
            "decodes disagree: {from_large:?} vs {from_small:?}"
        );
    }

    #[test]
    fn a_zoomed_camera_keeps_the_clicked_source_pixel_under_the_wave() {
        let source = (1920.0, 1080.0);
        let image = (960, 540);
        let pixels = gradient(image);
        let click = (1000.0, 500.0);
        let ripple = frame_ripple(source, &[(click.0, click.1, 400.0)]);
        assert_eq!(
            ripple.view,
            (0.0, 0.0, source.0, source.1),
            "the texture spans the whole encoded frame; the camera crops later"
        );
        let warped = warp_frame(&ripple, &pixels, image);
        let texture_pixel = (
            (click.0 / source.0 * f64::from(image.0)) as u32,
            (click.1 / source.1 * f64::from(image.1)) as u32,
        );
        let clicked_source = texture_source(&warped, image, source, texture_pixel);
        let zoomed_view = zoom::view_rect(2.0, (960.0, 540.0), source);
        let staged_pixel = zoom::source_to_stage(clicked_source, zoomed_view, 640.0, 480.0);
        let staged_click = zoom::source_to_stage(click, zoomed_view, 640.0, 480.0);
        assert!(
            (staged_pixel.0 - staged_click.0).abs() < 2.5
                && (staged_pixel.1 - staged_click.1).abs() < 2.5,
            "texture pixel {texture_pixel:?} reaches {staged_pixel:?}, click reaches {staged_click:?}"
        );
    }
}
