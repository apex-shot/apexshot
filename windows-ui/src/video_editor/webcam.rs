//! The webcam layer over a recording.
//!
//! The Windows recorder writes the camera to its own track and never into the
//! footage, so the layer is rebuilt here from the sidecar: the bubble's
//! fractional rect, its shape and its mirror, drawn over the picture at every
//! output size. The preview and the export share the drawing code, which is
//! what keeps what the editor shows and what it writes the same picture.
//!
//! The camera's clock is not the desktop's — a busy machine makes the recorder
//! drop camera frames without touching the screen's timeline — so a video
//! instant is mapped onto the track by its recorded length. The track freezes
//! on its last frame past the end, the way a still camera would.

use crate::recording::editor::sidecar::{PointerSidecar, WebcamTrack};
use gtk4::cairo::{Context, Format, ImageSurface};
use gtk4::glib;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How close two requests have to be before the second one is the same frame.
const REQUEST_STEP_SECONDS: f64 = 1.0 / 60.0;
/// How long the still preview waits for a decode before repainting.
const STILL_REPAINT_DELAY: Duration = Duration::from_millis(120);
/// The preview decodes at this width; the bubble is never larger on screen.
const PREVIEW_WIDTH: u32 = 720;

#[derive(Debug, Clone, Copy)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The bubble's place inside the video rectangle, from the sidecar's fractions.
pub(super) fn bubble_rect(video: Rect, track: &WebcamTrack) -> Rect {
    let (x, y, w, h) = track.rect();
    Rect {
        x: video.x + x * video.width,
        y: video.y + y * video.height,
        width: w * video.width,
        height: h * video.height,
    }
}

/// Add the bubble's outline to the context's current path. A circle is an
/// ellipse inscribed in the rect, everything else is a rounded rectangle whose
/// radius follows the shared shape table.
pub(super) fn add_shape_path(context: &Context, rect: Rect, shape: u8) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    if shape == 0 {
        let _ = context.save();
        context.translate(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        context.scale(rect.width / 2.0, rect.height / 2.0);
        context.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
        let _ = context.restore();
        return;
    }
    // Square keeps a tight radius, the taller shapes a softer one, matching the
    // bubble window the recording showed.
    let radius = rect.width * if shape == 1 { 0.04 } else { 0.06 };
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0);
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    context.new_path();
    context.move_to(x + radius, y);
    context.line_to(x + w - radius, y);
    context.curve_to(x + w, y, x + w, y, x + w, y + radius);
    context.line_to(x + w, y + h - radius);
    context.curve_to(x + w, y + h, x + w, y + h, x + w - radius, y + h);
    context.line_to(x + radius, y + h);
    context.curve_to(x, y + h, x, y + h, x, y + h - radius);
    context.line_to(x, y + radius);
    context.curve_to(x, y, x, y, x + radius, y);
    context.close_path();
}

/// Draw the bubble over the current context. The image is stretched to the
/// bubble's rect, which the recorder already cut to the bubble's own aspect.
pub(super) fn draw_bubble(
    context: &Context,
    rect: Rect,
    shape: u8,
    flip: bool,
    image: &ImageSurface,
) {
    let (image_width, image_height) = (f64::from(image.width()), f64::from(image.height()));
    if image_width <= 0.0 || image_height <= 0.0 {
        return;
    }
    let _ = context.save();
    add_shape_path(context, rect, shape);
    context.clip_preserve();
    {
        let _ = context.save();
        context.translate(rect.x, rect.y);
        if flip {
            context.translate(rect.width, 0.0);
            context.scale(-1.0, 1.0);
        }
        context.scale(rect.width / image_width, rect.height / image_height);
        let _ = context.set_source_surface(image, 0.0, 0.0);
        let _ = context.paint();
        let _ = context.restore();
    }
    context.set_line_width(1.5);
    context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
    let _ = context.stroke();
    let _ = context.restore();
}

pub(super) struct Frame {
    width: u32,
    height: u32,
    stamp: u64,
    pixels: Vec<u8>,
}

struct Job {
    path: PathBuf,
    seconds: f64,
    /// The decode size, already cut to the bubble's aspect so the surface can
    /// be stretched to the bubble's rect without distortion.
    size: (u32, u32),
}

struct Shared {
    job: Mutex<Option<Job>>,
    frame: Mutex<Option<Frame>>,
    stamp: Mutex<u64>,
}

struct Loaded {
    path: PathBuf,
    track: WebcamTrack,
    video_seconds: f64,
}

struct State {
    shared: Arc<Shared>,
    loaded: RefCell<Option<Loaded>>,
    cache: RefCell<Option<(u64, ImageSurface)>>,
    requested: Cell<f64>,
    repaint: Rc<dyn Fn()>,
}

/// The editor's camera layer: what to draw, from which track, and when.
#[derive(Clone)]
pub(super) struct WebcamModel {
    state: Rc<State>,
}

impl WebcamModel {
    pub fn new(repaint: Rc<dyn Fn()>) -> Self {
        let shared = Arc::new(Shared {
            job: Mutex::new(None),
            frame: Mutex::new(None),
            stamp: Mutex::new(0),
        });
        let worker = shared.clone();
        thread::spawn(move || decode_loop(worker));
        Self {
            state: Rc::new(State {
                shared,
                loaded: RefCell::new(None),
                cache: RefCell::new(None),
                requested: Cell::new(-1.0),
                repaint,
            }),
        }
    }

    /// Point the layer at a document. A recording without a camera, or whose
    /// track has gone missing, simply has no layer.
    pub fn load(&self, video: Option<&Path>, video_seconds: f64) {
        let mut loaded = self.state.loaded.borrow_mut();
        *loaded = None;
        *self.state.cache.borrow_mut() = None;
        self.state.requested.set(-1.0);
        *self.state.shared.frame.lock().unwrap() = None;
        *self.state.shared.job.lock().unwrap() = None;

        let Some(video) = video else { return };
        let Some(sidecar) = PointerSidecar::load_next_to_video(video) else {
            return;
        };
        let Some(track) = sidecar.webcam.clone().filter(WebcamTrack::is_valid) else {
            return;
        };
        let Some(path) = sidecar.webcam_path(video) else {
            return;
        };
        let video_seconds = if video_seconds > 0.0 {
            video_seconds
        } else {
            crate::recording::editor::ffmpeg::probe_duration(video).unwrap_or(0.0)
        };
        *loaded = Some(Loaded {
            path,
            track,
            video_seconds,
        });
    }

    /// Ask for the frame the playhead needs. The decode runs on a worker, and
    /// the still preview repaints once it lands; playback repaints anyway.
    pub fn request(&self, source_seconds: f64) {
        let target = {
            let loaded = self.state.loaded.borrow();
            let Some(loaded) = loaded.as_ref() else {
                return;
            };
            loaded
                .track
                .track_seconds(source_seconds, loaded.video_seconds)
        };
        if (self.state.requested.get() - target).abs() < REQUEST_STEP_SECONDS {
            return;
        }
        self.state.requested.set(target);

        {
            let loaded = self.state.loaded.borrow();
            let Some(loaded) = loaded.as_ref() else {
                return;
            };
            *self.state.shared.job.lock().unwrap() = Some(Job {
                path: loaded.path.clone(),
                seconds: target,
                size: preview_size(&loaded.track),
            });
        }

        let repaint = self.state.repaint.clone();
        glib::timeout_add_local_once(STILL_REPAINT_DELAY, move || repaint());
    }

    /// Draw the layer over the picture. The bubble is an overlay: it keeps its
    /// place in the frame while the footage zooms underneath it.
    pub fn draw(
        &self,
        context: &Context,
        allocation_width: i32,
        allocation_height: i32,
        source_size: Option<(u32, u32)>,
    ) {
        let Some((width, height)) = source_size else {
            return;
        };
        let Some(video) = super::cursor::video_rect_for(
            f64::from(allocation_width),
            f64::from(allocation_height),
            f64::from(width),
            f64::from(height),
        ) else {
            return;
        };
        let (track, rect) = {
            let loaded = self.state.loaded.borrow();
            let Some(loaded) = loaded.as_ref() else {
                return;
            };
            let rect = bubble_rect(
                Rect {
                    x: video.x,
                    y: video.y,
                    width: video.width,
                    height: video.height,
                },
                &loaded.track,
            );
            (loaded.track.clone(), rect)
        };
        if rect.width < 1.0 || rect.height < 1.0 {
            return;
        }

        let Some(image) = self.current_surface() else {
            return;
        };
        draw_bubble(context, rect, track.shape, track.flip, &image);
    }

    /// The newest decoded frame as a surface, rebuilt only when a new frame
    /// lands. cairo surfaces belong to the thread that draws them, so the
    /// worker hands over pixels and the surface is built here.
    fn current_surface(&self) -> Option<ImageSurface> {
        let guard = self.state.shared.frame.lock().unwrap();
        let frame = guard.as_ref()?;
        if let Some((stamp, surface)) = self.state.cache.borrow().as_ref() {
            if *stamp == frame.stamp {
                return Some(surface.clone());
            }
        }
        let stride = frame.width as usize * 4;
        let surface = ImageSurface::create_for_data(
            frame.pixels.clone(),
            Format::ARgb32,
            frame.width as i32,
            frame.height as i32,
            stride as i32,
        )
        .ok()?;
        *self.state.cache.borrow_mut() = Some((frame.stamp, surface.clone()));
        Some(surface)
    }
}

fn decode_loop(shared: Arc<Shared>) {
    loop {
        let job = shared.job.lock().unwrap().take();
        let Some(job) = job else {
            thread::sleep(Duration::from_millis(8));
            continue;
        };
        let Some((pixels, width, height)) = decode_frame(&job.path, job.seconds, job.size) else {
            continue;
        };
        let stamp = {
            let mut stamp = shared.stamp.lock().unwrap();
            *stamp += 1;
            *stamp
        };
        *shared.frame.lock().unwrap() = Some(Frame {
            width,
            height,
            stamp,
            pixels,
        });
    }
}

fn decode_frame(path: &Path, seconds: f64, size: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    let (width, height) = size;
    let frame_len = width as usize * height as usize * 4;
    let mut child = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-ss", &format!("{seconds:.6}")])
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1"])
        .args([
            "-vf",
            &format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"
            ),
        ])
        .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let mut buffer = vec![0u8; frame_len];
    let read = stdout.read_exact(&mut buffer).is_ok();
    let _ = child.kill();
    let _ = child.wait();
    read.then_some((buffer, width, height))
}

/// The preview decode size for a track: the widest the bubble is ever drawn,
/// at the track's own aspect.
fn preview_size(track: &WebcamTrack) -> (u32, u32) {
    if track.width == 0 || track.height == 0 {
        return (PREVIEW_WIDTH, PREVIEW_WIDTH * 9 / 16);
    }
    let height = (PREVIEW_WIDTH as u64 * track.height as u64 / track.width.max(1) as u64)
        .clamp(2, 4096) as u32;
    (PREVIEW_WIDTH, height)
}

/// A sequential reader for the export: frames are read in lock step with the
/// video's, so the two timelines cannot drift apart inside a span.
pub(super) struct TrackReader {
    child: Child,
    stdout: ChildStdout,
    size: (u32, u32),
    buffer: Vec<u8>,
    last: Vec<u8>,
    finished: bool,
}

impl TrackReader {
    pub fn open(
        path: &Path,
        start_seconds: f64,
        duration_seconds: f64,
        fps: f64,
        size: (u32, u32),
    ) -> Option<Self> {
        let frame_len = size.0 as usize * size.1 as usize * 4;
        let frames = (duration_seconds.max(0.0) * fps).ceil().max(1.0) as u64;
        let mut child = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-ss", &format!("{start_seconds:.6}")])
            .arg("-i")
            .arg(path)
            .args(["-t", &format!("{duration_seconds:.6}")])
            .args([
                "-vf",
                &format!(
                    "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},fps={fps}",
                    size.0, size.1, size.0, size.1
                ),
            ])
            .args(["-frames:v", &frames.to_string()])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        Some(Self {
            child,
            stdout,
            size,
            buffer: vec![0u8; frame_len],
            last: Vec::new(),
            finished: false,
        })
    }

    /// The next frame, or the last one again when the track has run out, which
    /// is what a camera that stopped early should look like.
    pub fn next_frame(&mut self) -> Option<(&[u8], (u32, u32))> {
        if !self.finished {
            if self.stdout.read_exact(&mut self.buffer).is_ok() {
                self.last.clone_from(&self.buffer);
                return Some((&self.last, self.size));
            }
            self.finished = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        (!self.last.is_empty()).then_some((self.last.as_slice(), self.size))
    }
}

impl Drop for TrackReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Draw the bubble over a composed RGBA frame.
///
/// The layer holds the frame's own pixels under the bubble, so the antialiased
/// edge blends into the footage and the result can be copied back whole. Only
/// the bubble's box is ever touched, so the cost follows the bubble and not the
/// frame size.
pub(super) fn composite_into(
    pixels: &mut [u8],
    size: (u32, u32),
    track: &WebcamTrack,
    frame: &[u8],
    frame_size: (u32, u32),
) {
    if size.0 == 0 || size.1 == 0 || frame_size.0 == 0 || frame_size.1 == 0 {
        return;
    }
    let rect = bubble_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(size.0),
            height: f64::from(size.1),
        },
        track,
    );
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = (rect.x + rect.width).round().clamp(0.0, f64::from(size.0)) as u32;
    let y1 = (rect.y + rect.height).round().clamp(0.0, f64::from(size.1)) as u32;
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width < 2 || height < 2 {
        return;
    }

    let Ok(mut layer) = ImageSurface::create(Format::ARgb32, width as i32, height as i32) else {
        return;
    };
    let frame_stride = size.0 as usize * 4;
    let stride = layer.stride() as usize;
    {
        let Ok(mut data) = layer.data() else {
            return;
        };
        for y in 0..height as usize {
            for x in 0..width as usize {
                let source = (y0 as usize + y) * frame_stride + (x0 as usize + x) * 4;
                let target = y * stride + x * 4;
                let pixel = &pixels[source..source + 4];
                data[target] = pixel[2];
                data[target + 1] = pixel[1];
                data[target + 2] = pixel[0];
                data[target + 3] = 255;
            }
        }
    }
    drop(layer.data());

    let Ok(image) = ImageSurface::create_for_data(
        frame.to_vec(),
        Format::ARgb32,
        frame_size.0 as i32,
        frame_size.1 as i32,
        frame_size.0 as i32 * 4,
    ) else {
        return;
    };
    {
        let Ok(context) = Context::new(&layer) else {
            return;
        };
        draw_bubble(
            &context,
            Rect {
                x: 0.0,
                y: 0.0,
                width: f64::from(width),
                height: f64::from(height),
            },
            track.shape,
            track.flip,
            &image,
        );
    }
    layer.mark_dirty();
    {
        let Ok(data) = layer.data() else {
            return;
        };
        for y in 0..height as usize {
            for x in 0..width as usize {
                let target = (y0 as usize + y) * frame_stride + (x0 as usize + x) * 4;
                let source = y * stride + x * 4;
                pixels[target] = data[source + 2];
                pixels[target + 1] = data[source + 1];
                pixels[target + 2] = data[source];
                pixels[target + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::cairo::Context;

    fn track(x: f64, y: f64, w: f64, h: f64, shape: u8) -> WebcamTrack {
        WebcamTrack {
            file: "track.mp4".to_string(),
            width: 640,
            height: 360,
            duration: 2.0,
            x,
            y,
            w,
            h,
            shape,
            flip: false,
        }
    }

    fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Vec<u8> {
        bgra.iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect()
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = (y as usize * width as usize + x as usize) * 4;
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }

    #[test]
    fn the_bubble_sits_on_its_fractions_of_the_picture() {
        let video = Rect {
            x: 20.0,
            y: 40.0,
            width: 1000.0,
            height: 500.0,
        };
        let rect = bubble_rect(video, &track(0.5, 0.25, 0.2, 0.3, 1));
        assert!((rect.x - 520.0).abs() < 1e-9);
        assert!((rect.y - 165.0).abs() < 1e-9);
        assert!((rect.width - 200.0).abs() < 1e-9);
        assert!((rect.height - 150.0).abs() < 1e-9);
    }

    #[test]
    fn the_track_pixels_land_inside_the_bubble_and_nowhere_else() {
        let mut frame = solid(400, 400, [30, 20, 10, 255]);
        let camera = solid(100, 100, [0, 255, 0, 255]);
        composite_into(
            &mut frame,
            (400, 400),
            &track(0.6, 0.6, 0.3, 0.3, 1),
            &camera,
            (100, 100),
        );
        // Centre of the bubble: 0.6..0.9 of the frame.
        let inside = pixel(&frame, 400, 300, 300);
        assert!(inside[1] > 200, "camera pixel expected, got {inside:?}");
        assert!(inside[0] < 40);
        // Outside it, the footage is untouched.
        assert_eq!(pixel(&frame, 400, 10, 10), [30, 20, 10, 255]);
        assert_eq!(pixel(&frame, 400, 380, 60), [30, 20, 10, 255]);
    }

    #[test]
    fn a_circle_leaves_the_corners_of_its_box_alone() {
        let mut frame = solid(200, 200, [10, 10, 10, 255]);
        let camera = solid(64, 64, [0, 255, 0, 255]);
        composite_into(
            &mut frame,
            (200, 200),
            &track(0.25, 0.25, 0.5, 0.5, 0),
            &camera,
            (64, 64),
        );
        // The box runs 50..150; its top-left corner is outside the circle.
        assert_eq!(pixel(&frame, 200, 52, 52), [10, 10, 10, 255]);
        assert!(pixel(&frame, 200, 100, 100)[1] > 200);
    }

    #[test]
    fn the_preview_draws_the_bubble_through_its_shape() {
        let mut surface = ImageSurface::create(Format::ARgb32, 200, 200).unwrap();
        let camera = ImageSurface::create_for_data(
            solid(32, 32, [0, 0, 255, 255]),
            Format::ARgb32,
            32,
            32,
            32 * 4,
        )
        .unwrap();
        {
            let context = Context::new(&surface).unwrap();
            draw_bubble(
                &context,
                Rect {
                    x: 50.0,
                    y: 50.0,
                    width: 100.0,
                    height: 100.0,
                },
                0,
                false,
                &camera,
            );
        }
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let alpha_at = |x: usize, y: usize| data[y * stride + x * 4 + 3];
        assert_eq!(alpha_at(52, 52), 0, "the circle's corner stays empty");
        assert!(alpha_at(100, 100) > 200, "the middle is painted");
    }

    #[test]
    fn a_mirrored_bubble_flips_only_the_picture() {
        // The camera's left half is red and its right half blue, so after a
        // mirror the bubble shows blue on the left.
        let mut camera = Vec::new();
        for _ in 0..16 {
            for x in 0..32 {
                let colour = if x < 16 {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                };
                camera.extend_from_slice(&colour);
            }
        }
        let mut frame = solid(100, 100, [0, 0, 0, 255]);
        let mut mirrored = track(0.0, 0.0, 1.0, 1.0, 1);
        mirrored.flip = true;
        composite_into(&mut frame, (100, 100), &mirrored, &camera, (32, 16));
        let left = pixel(&frame, 100, 10, 50);
        assert!(
            left[0] < 40 && left[2] > 200,
            "left side should now be the camera's right half, blue: {left:?}"
        );
    }
}
