//! Export the composition to an H.264 MP4.
//!
//! The frame plan walks the kept source spans (trim minus cuts) in composition
//! order, and every frame is composited by the same code the preview uses:
//! the follow camera, the click ripple warp, and the cursor sprite with its
//! press spring. Presentation effects that only make sense in the live stage —
//! background, frame aspect, rounded corners, motion blur, audio — are not
//! part of this path.

use super::cursor::preview_ripple;
use super::pixels::{Nv12Converter, YuvSpace};
use super::press::press_cursor_scale;
use super::timeline::ZoomClip;
use super::webcam;
use super::zoom::{self, TimeMapping, ViewRect, ZoomCameraCache};
use crate::recording::editor::cursor_sprite;
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::ClickEffect;
use crate::recording::editor::model::CursorSettings;
use crate::recording::editor::sidecar::{CursorMotion, PointerSidecar, WebcamTrack};
use gtk4::cairo::{Context, Format, ImageSurface};
use rayon::prelude::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

const DEFAULT_FPS: f64 = 30.0;

/// Everything the exporter needs that only the window knows.
pub(super) struct ExportModel {
    pub source: PathBuf,
    pub source_size: (u32, u32),
    pub trim_start: f64,
    pub trim_end: f64,
    pub cuts: Vec<f64>,
    pub zooms: Vec<ZoomClip>,
    /// The camera settings the preview is using, so the export's camera is the
    /// same camera.
    pub zoom_camera: zoom::ZoomCameraSettings,
    pub cursor: CursorSettings,
    pub sidecar: Option<PointerSidecar>,
    pub crf: u32,
}

/// One contiguous kept span of footage, rendered back to back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceSpan {
    pub composition_start: f64,
    pub source_start: f64,
    pub frames: u64,
}

/// Split the trimmed footage at the cuts, rounding frame counts on the shared
/// grid so the spans join without a gap, a repeat, or frame drift.
pub(super) fn frame_plan(
    trim_start: f64,
    trim_end: f64,
    cuts: &[f64],
    fps: f64,
) -> Vec<SourceSpan> {
    let start = trim_start.max(0.0);
    let end = trim_end.max(start);
    if !fps.is_finite() || fps <= 0.0 || end - start <= 1e-9 {
        return Vec::new();
    }
    let total = ((end - start) * fps).round() as u64;
    if total == 0 {
        return Vec::new();
    }
    // Every seek and every sample sits on the recording's frame grid: the trim
    // edge and the cuts each snap to the nearest frame, so a seam is exactly
    // one frame step and never repeats or skips footage.
    let origin = (start * fps).round() / fps;
    let mut cut_frames: Vec<u64> = cuts
        .iter()
        .copied()
        .filter(|cut| cut.is_finite() && *cut > start + 1e-9 && *cut < end - 1e-9)
        .map(|cut| ((cut - origin) * fps).round() as u64)
        .filter(|frame| *frame > 0 && *frame < total)
        .collect();
    cut_frames.sort_unstable();
    cut_frames.dedup();
    let boundaries: Vec<u64> = std::iter::once(0)
        .chain(cut_frames)
        .chain(std::iter::once(total))
        .collect();
    boundaries
        .windows(2)
        .filter_map(|pair| {
            let frames = pair[1] - pair[0];
            (frames > 0).then_some(SourceSpan {
                composition_start: pair[0] as f64 / fps,
                source_start: origin + pair[0] as f64 / fps,
                frames,
            })
        })
        .collect()
}

pub(super) fn total_frames(spans: &[SourceSpan]) -> u64 {
    spans.iter().map(|span| span.frames).sum()
}

/// Source time of a global composition frame, through the span that owns it.
pub(super) fn frame_source_time(spans: &[SourceSpan], frame: u64, fps: f64) -> Option<f64> {
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    let mut remaining = frame;
    for span in spans {
        if remaining < span.frames {
            return Some(span.source_start + remaining as f64 / fps);
        }
        remaining -= span.frames;
    }
    None
}

/// Export dimensions: the source size, with even edges for yuv420p.
pub(super) fn output_size(source: (u32, u32)) -> (u32, u32) {
    (source.0.max(2) & !1, source.1.max(2) & !1)
}

pub(super) enum ExportUpdate {
    Progress { done: u64, total: u64 },
    Finished(PathBuf),
    Failed(String),
    Cancelled,
}

pub(super) struct ExportHandle {
    pub receiver: Receiver<ExportUpdate>,
    cancel: Arc<AtomicBool>,
}

impl ExportHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(super) fn start_export(model: ExportModel, output: PathBuf) -> ExportHandle {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        let encoder = Encoder::fastest();
        let mut result = render(&model, &output, &sender, &thread_cancel, encoder);
        if encoder != Encoder::X264 {
            if let Err(End::Failed(message)) = &result {
                eprintln!(
                    "[export] {} failed ({message}); retrying with libx264",
                    encoder.codec()
                );
                result = render(&model, &output, &sender, &thread_cancel, Encoder::X264);
            }
        }
        match result {
            Ok(()) => {
                let _ = sender.send(ExportUpdate::Finished(output));
            }
            Err(End::Cancelled) => {
                let _ = std::fs::remove_file(&output);
                let _ = sender.send(ExportUpdate::Cancelled);
            }
            Err(End::Failed(message)) => {
                let _ = std::fs::remove_file(&output);
                let _ = sender.send(ExportUpdate::Failed(message));
            }
        }
    });
    ExportHandle { receiver, cancel }
}

/// The H.264 encoders an export can use, in the order they are tried: the
/// GPU's own encoder when the machine has one that opens, then x264 on the
/// CPU, which also takes over if a hardware encoder fails mid-export.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoder {
    Nvenc,
    QuickSync,
    Amf,
    X264,
}

impl Encoder {
    /// The fastest encoder this machine can open, probed once per process.
    fn fastest() -> Self {
        static FASTEST: OnceLock<Encoder> = OnceLock::new();
        *FASTEST.get_or_init(|| {
            let chosen = [Self::Nvenc, Self::QuickSync, Self::Amf]
                .into_iter()
                .find(|encoder| encoder.opens())
                .unwrap_or(Self::X264);
            eprintln!("[export] encoder: {}", chosen.codec());
            chosen
        })
    }

    fn codec(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::QuickSync => "h264_qsv",
            Self::Amf => "h264_amf",
            Self::X264 => "libx264",
        }
    }

    /// The codec, constant-quality rate control at the editor's CRF, and the
    /// pixel format the encoder takes without another conversion.
    fn args(self, crf: u32) -> Vec<String> {
        let crf = crf.to_string();
        let crf = crf.as_str();
        let settings: Vec<&str> = match self {
            Self::Nvenc => vec![
                "-preset", "p4", "-tune", "hq", "-rc", "vbr", "-cq", crf, "-b:v", "0", "-pix_fmt",
                "nv12",
            ],
            Self::QuickSync => vec![
                "-preset",
                "medium",
                "-global_quality",
                crf,
                "-pix_fmt",
                "nv12",
            ],
            Self::Amf => vec![
                "-quality", "balanced", "-rc", "cqp", "-qp_i", crf, "-qp_p", crf, "-qp_b", crf,
                "-pix_fmt", "nv12",
            ],
            Self::X264 => vec!["-preset", "veryfast", "-crf", crf, "-pix_fmt", "yuv420p"],
        };
        ["-c:v", self.codec()]
            .into_iter()
            .chain(settings)
            .map(str::to_string)
            .collect()
    }

    /// Whether ffmpeg can open this encoder here: a build lists every GPU
    /// encoder it was compiled with, whether or not the machine has the
    /// hardware or a new enough driver for it.
    fn opens(self) -> bool {
        Command::new(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-f", "lavfi", "-i", "color=c=black:s=320x240:r=30"])
            .args(["-frames:v", "2"])
            .args(self.args(23))
            .args(["-f", "null", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

enum End {
    Cancelled,
    Failed(String),
}

fn render(
    model: &ExportModel,
    output: &Path,
    sender: &Sender<ExportUpdate>,
    cancel: &Arc<AtomicBool>,
    encoder: Encoder,
) -> Result<(), End> {
    let fps = ffmpeg::probe_frame_rate(&model.source)
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(DEFAULT_FPS);
    let spans = frame_plan(model.trim_start, model.trim_end, &model.cuts, fps);
    let total = total_frames(&spans);
    if total == 0 {
        return Err(End::Failed("there is no footage to export".to_string()));
    }
    if model.source_size.0 == 0 || model.source_size.1 == 0 {
        return Err(End::Failed("the recording has no frame size".to_string()));
    }
    let source = (
        f64::from(model.source_size.0),
        f64::from(model.source_size.1),
    );
    let size = output_size(model.source_size);
    let mapping = TimeMapping::new(
        spans
            .iter()
            .map(|span| {
                let duration = span.frames as f64 / fps;
                (
                    span.composition_start,
                    span.composition_start + duration,
                    span.source_start,
                    span.source_start + duration,
                )
            })
            .collect(),
    );

    let probed = ffmpeg::probe_color_space(&model.source);
    let converter = Nv12Converter::new(YuvSpace::from_probe(
        probed.0.as_deref(),
        probed.1.as_deref(),
    ));
    let mut encoder = spawn_encoder(output, size, fps, model.crf, encoder)?;
    let encoder_input = encoder
        .stdin
        .take()
        .ok_or_else(|| End::Failed("the encoder did not open its input".to_string()))?;
    let frame_bytes = size.0 as usize * size.1 as usize * 3 / 2;
    let pipe = EncoderPipe::new(encoder_input, frame_bytes, pipe_depth(frame_bytes));
    let mut camera = ZoomCameraCache::default();
    let mut decoded = vec![0u8; model.source_size.0 as usize * model.source_size.1 as usize * 4];
    let mut packed = vec![0u8; model.source_size.0 as usize * model.source_size.1 as usize * 3 / 2];
    let mut scaled = vec![0u8; size.0 as usize * size.1 as usize * 4];
    let mut cursor_layer = match &model.sidecar {
        Some(_) => Some(
            ImageSurface::create(Format::ARgb32, size.0 as i32, size.1 as i32).map_err(
                |error| End::Failed(format!("could not allocate the cursor layer: {error}")),
            )?,
        ),
        None => None,
    };
    let webcam = model.sidecar.as_ref().and_then(|sidecar| {
        let track = sidecar.webcam.clone().filter(WebcamTrack::is_valid)?;
        let path = sidecar.webcam_path(&model.source)?;
        Some((track, path))
    });
    let video_seconds = webcam
        .as_ref()
        .map(|_| ffmpeg::probe_duration(&model.source).unwrap_or(0.0))
        .unwrap_or(0.0);
    let mut done = 0u64;
    let started = Instant::now();

    for span in &spans {
        let (mut decoder, mut stdout) =
            match spawn_decoder(&model.source, *span, fps, model.source_size) {
                Ok(mut decoder) => match decoder.stdout.take() {
                    Some(stdout) => (decoder, stdout),
                    None => {
                        stop(&mut encoder);
                        return Err(End::Failed(
                            "the decoder did not open its output".to_string(),
                        ));
                    }
                },
                Err(error) => {
                    stop(&mut encoder);
                    return Err(error);
                }
            };
        let mut track_reader = webcam.as_ref().and_then(|(track, path)| {
            let start = track.track_seconds(span.source_start, video_seconds);
            let duration = span.frames as f64 / fps;
            let decode = (
                (track.w * f64::from(size.0)).round().clamp(2.0, 4096.0) as u32,
                (track.h * f64::from(size.1)).round().clamp(2.0, 4096.0) as u32,
            );
            webcam::TrackReader::open(path, start, duration, fps, decode)
        });
        let mut span_frame = 0u64;
        while span_frame < span.frames {
            if cancel.load(Ordering::Relaxed) {
                let _ = decoder.kill();
                let _ = decoder.wait();
                stop(&mut encoder);
                return Err(End::Cancelled);
            }
            if stdout.read_exact(&mut packed).is_err() {
                break;
            }
            converter.to_rgba_into(&packed, model.source_size, &mut decoded);
            let source_time = frame_source_time(&spans, done, fps)
                .ok_or_else(|| End::Failed("the frame plan ran out of frames".to_string()))?;
            let composition_time = done as f64 / fps;
            span_frame += 1;
            let camera_value = camera.evaluate(
                &model.zooms,
                source,
                &mapping,
                model.zoom_camera,
                composition_time,
                || model.sidecar.clone(),
            );
            let view = zoom::view_rect(camera_value.0, camera_value.1, source);
            let cropped = !(view_is_full(view, source) && size == model.source_size);
            if cropped {
                crop_scale_into(&decoded, model.source_size, view, size, &mut scaled);
            }
            let base: &[u8] = if cropped { &scaled } else { &decoded };

            let ripple = model.sidecar.as_ref().and_then(|sidecar| {
                preview_ripple(sidecar, model.cursor, source_time, model.source_size, view)
            });
            let mut warped = None;
            if let Some(ripple) = &ripple {
                let pixels = ripple.warp_pixels(
                    base,
                    size,
                    (0.0, 0.0, f64::from(size.0), f64::from(size.1)),
                    size,
                );
                if pixels.len() == base.len() {
                    warped = Some(pixels);
                }
            }
            let frame: &mut [u8] = if let Some(warped) = warped.as_mut() {
                warped
            } else if cropped {
                &mut scaled
            } else {
                &mut decoded
            };

            if let (Some(sidecar), Some(layer)) = (&model.sidecar, cursor_layer.as_mut()) {
                draw_cursor(
                    frame,
                    size,
                    view,
                    sidecar,
                    model.cursor,
                    source_time,
                    model.source_size,
                    layer,
                );
            }
            // The camera layer sits over the cursor, the way the bubble window
            // sat over the desktop while the recording ran.
            if let (Some((track, _)), Some(reader)) = (webcam.as_ref(), track_reader.as_mut()) {
                if let Some((camera, camera_size)) = reader.next_frame() {
                    webcam::composite_into(frame, size, track, camera, camera_size);
                }
            }

            let mut nv12 = pipe.buffer();
            converter.convert_into(frame, size, &mut nv12);
            if let Err(error) = pipe.submit(nv12) {
                let _ = decoder.kill();
                let _ = decoder.wait();
                stop(&mut encoder);
                return Err(error);
            }
            done += 1;
            if done % 100 == 0 || done == total {
                let elapsed = started.elapsed().as_secs_f64();
                eprintln!(
                    "[export] frame {done}/{total} at {:.1} ms/frame",
                    elapsed * 1000.0 / done as f64
                );
            }
            let _ = sender.send(ExportUpdate::Progress { done, total });
        }
        let _ = decoder.kill();
        let _ = decoder.wait();
        if span_frame < span.frames {
            stop(&mut encoder);
            return Err(End::Failed(
                "the recording ended before the planned frames".to_string(),
            ));
        }
    }

    if cancel.load(Ordering::Relaxed) {
        stop(&mut encoder);
        return Err(End::Cancelled);
    }
    if let Err(error) = pipe.finish() {
        stop(&mut encoder);
        return Err(error);
    }
    let result = encoder
        .wait_with_output()
        .map_err(|error| End::Failed(format!("the encoder did not finish: {error}")))?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr);
        return Err(End::Failed(format!(
            "the encoder failed: {}",
            detail.trim()
        )));
    }
    Ok(())
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_decoder(
    source: &Path,
    span: SourceSpan,
    fps: f64,
    size: (u32, u32),
) -> Result<Child, End> {
    let duration = span.frames as f64 / fps;
    Command::new(ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-ss", &format!("{:.6}", span.source_start)])
        .arg("-i")
        .arg(source)
        .args(["-t", &format!("{duration:.6}")])
        .arg("-an")
        .args(["-vf", &format!("scale={}:{},fps={fps}", size.0, size.1)])
        .args(["-frames:v", &span.frames.to_string()])
        .args(["-f", "rawvideo", "-pix_fmt", "nv12", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| End::Failed(format!("could not start decoding: {error}")))
}

fn spawn_encoder(
    output: &Path,
    size: (u32, u32),
    fps: f64,
    crf: u32,
    encoder: Encoder,
) -> Result<Child, End> {
    Command::new(ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            "nv12",
            "-s",
            &format!("{}x{}", size.0, size.1),
            "-r",
            &format!("{fps:.6}"),
            "-i",
            "pipe:0",
        ])
        .arg("-an")
        .args(encoder.args(crf))
        .args(["-movflags", "+faststart"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| End::Failed(format!("could not start the encoder: {error}")))
}

/// How many finished frames may wait for the encoder.
///
/// x264 wants frames faster than the compositor can usually make them, so a
/// short queue is enough to keep both sides busy; the cap keeps the memory a
/// 4K export holds for buffering around 25 MB rather than a frame apiece for
/// every core.
fn pipe_depth(frame_bytes: usize) -> usize {
    const BUDGET: usize = 32 * 1024 * 1024;
    (BUDGET / frame_bytes.max(1)).clamp(2, 4)
}

/// Hands finished frames to the encoder from a writer thread.
///
/// Writing on the compositor thread would make the two stages take turns:
/// a Windows pipe holds about 64 KB, so `write_all` blocks as soon as the
/// encoder is a frame behind. A writer thread and a small pool of recycled
/// buffers let the encoder drain frame N while the compositor builds N+1.
struct EncoderPipe {
    queue: Option<mpsc::SyncSender<Vec<u8>>>,
    recycled: Receiver<Vec<u8>>,
    writer: Option<std::thread::JoinHandle<std::io::Result<()>>>,
    frame_bytes: usize,
}

impl EncoderPipe {
    fn new<W: Write + Send + 'static>(mut input: W, frame_bytes: usize, depth: usize) -> Self {
        let (queue, pending) = mpsc::sync_channel::<Vec<u8>>(depth);
        let (back, recycled) = mpsc::channel::<Vec<u8>>();
        let writer = std::thread::spawn(move || {
            while let Ok(frame) = pending.recv() {
                input.write_all(&frame)?;
                let _ = back.send(frame);
            }
            input.flush()
        });
        Self {
            queue: Some(queue),
            recycled,
            writer: Some(writer),
            frame_bytes,
        }
    }

    /// A buffer to composite into: a recycled one when the writer has handed
    /// one back, otherwise a fresh allocation. At most `depth + 1` exist.
    fn buffer(&self) -> Vec<u8> {
        self.recycled
            .try_recv()
            .unwrap_or_else(|_| vec![0u8; self.frame_bytes])
    }

    /// Queue a finished frame, blocking while the encoder is `depth` frames
    /// behind.
    fn submit(&self, frame: Vec<u8>) -> Result<(), End> {
        let Some(queue) = self.queue.as_ref() else {
            return Err(End::Failed(
                "the encoder stopped accepting frames".to_string(),
            ));
        };
        queue
            .send(frame)
            .map_err(|_| End::Failed("the encoder stopped accepting frames".to_string()))
    }

    /// Close the pipe and wait for the writer, reporting the first write
    /// error the encoder caused.
    fn finish(mut self) -> Result<(), End> {
        self.queue.take();
        match self.writer.take().map(|writer| writer.join()) {
            Some(Ok(Ok(()))) | None => Ok(()),
            Some(Ok(Err(error))) => {
                Err(End::Failed(format!("could not feed the encoder: {error}")))
            }
            Some(Err(_)) => Err(End::Failed("the encoder writer panicked".to_string())),
        }
    }
}

impl Drop for EncoderPipe {
    fn drop(&mut self) {
        self.queue.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn view_is_full(view: ViewRect, source: (f64, f64)) -> bool {
    view.0 <= 0.001
        && view.1 <= 0.001
        && view.0 + view.2 >= source.0 - 0.001
        && view.1 + view.3 >= source.1 - 0.001
}

/// Bilinear crop from the source frame to the camera view, at export size.
///
/// Rows are independent, so the whole pass runs in parallel over the one
/// reused output buffer. The weights are fixed point rather than `f64`: at 4K
/// the float version spent more time converting bytes to doubles than sampling
/// them, and 16-bit weights land on the same pixel a float bilinear does to
/// within one level. Alpha is written as opaque because the decoder only ever
/// produces opaque frames.
fn crop_scale_into(
    pixels: &[u8],
    size: (u32, u32),
    view: ViewRect,
    out: (u32, u32),
    output: &mut [u8],
) {
    const ONE: i64 = 1 << 16;
    const ROUND: i64 = (ONE * ONE) / 2;
    let width = size.0 as usize;
    let height = size.1 as usize;
    let out_width = out.0 as usize;
    let step_x = view.2 / f64::from(out.0);
    let step_y = view.3 / f64::from(out.1);
    // The horizontal taps are the same for every row, so each column's
    // neighbours and weight are computed once instead of per sampled pixel.
    let columns: Vec<(usize, usize, i64)> = (0..out_width)
        .map(|x| {
            let source_x = (view.0 + (x as f64 + 0.5) * step_x - 0.5)
                .clamp(0.0, width.saturating_sub(1) as f64);
            let x0 = source_x as usize;
            (
                x0,
                (x0 + 1).min(width - 1),
                ((source_x - x0 as f64) * ONE as f64).round() as i64,
            )
        })
        .collect();
    output
        .par_chunks_mut(out_width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let source_y = (view.1 + (y as f64 + 0.5) * step_y - 0.5)
                .clamp(0.0, height.saturating_sub(1) as f64);
            let y0 = source_y as usize;
            let y1 = (y0 + 1).min(height - 1);
            let fy = ((source_y - y0 as f64) * ONE as f64).round() as i64;
            let inv_y = ONE - fy;
            let top_row = &pixels[y0 * width * 4..];
            let bottom_row = &pixels[y1 * width * 4..];
            for (x, &(x0, x1, fx)) in columns.iter().enumerate() {
                let (left, right) = (x0 * 4, x1 * 4);
                let inv_x = ONE - fx;
                let offset = x * 4;
                for channel in 0..3 {
                    let top = i64::from(top_row[left + channel]) * inv_x
                        + i64::from(top_row[right + channel]) * fx;
                    let bottom = i64::from(bottom_row[left + channel]) * inv_x
                        + i64::from(bottom_row[right + channel]) * fx;
                    row[offset + channel] = ((top * inv_y + bottom * fy + ROUND) >> 32) as u8;
                }
                row[offset + 3] = 255;
            }
        });
}

/// Half the width of the largest thing the cursor overlay can paint, in stage
/// pixels: the sprite (art, press scale, shadow) or the widest click ring.
/// Drawing and copying stay inside this box instead of the whole frame.
fn overlay_half_extent(settings: CursorSettings, reference_width: f64) -> f64 {
    let scale = settings.size * settings.click_scale;
    let sprite = 80.0 * settings.size + 8.0;
    let ring = match settings.click_effect {
        ClickEffect::Circle => reference_width * 0.031 + 4.0,
        ClickEffect::Spotlight => 30.0 * scale + 8.0,
        ClickEffect::Echo => 44.0 * scale + 8.0,
        ClickEffect::None | ClickEffect::Ripple => 0.0,
    };
    sprite.max(ring)
}

/// Draw the cursor, its press scale and any non-ripple click effect onto the
/// composited frame, exactly as the preview's overlay does.
///
/// The cairo surface is reused for the whole export and only the box the
/// sprite and its rings can touch is copied in, drawn, and copied back, so the
/// cost does not scale with the frame size.
fn draw_cursor(
    pixels: &mut [u8],
    size: (u32, u32),
    view: ViewRect,
    sidecar: &PointerSidecar,
    settings: CursorSettings,
    source_time: f64,
    source_size: (u32, u32),
    layer: &mut ImageSurface,
) {
    if !sidecar.can_render_cursor_overlay() {
        return;
    }
    let motion = CursorMotion {
        smooth: settings.smooth,
        hide_idle: settings.hide_idle,
        idle_ms: settings.idle_ms,
        speed: settings.speed,
    };
    let Some(frame) = sidecar.presented_in_video_at(
        source_time,
        motion,
        f64::from(source_size.0),
        f64::from(source_size.1),
    ) else {
        return;
    };
    let settings = settings.clamped();
    let map = |point| zoom::source_to_stage(point, view, f64::from(size.0), f64::from(size.1));
    let cursor_point = map((frame.x, frame.y));
    // A ripple is a footage warp in the export just as in the preview, so its
    // ring is never drawn; every other effect is.
    let rings: Vec<(f64, f64, f64)> = if settings.click_effect == ClickEffect::Ripple {
        Vec::new()
    } else {
        sidecar
            .click_ripples_in_video_at(
                source_time,
                settings.click_window_seconds(),
                f64::from(source_size.0),
                f64::from(source_size.1),
            )
            .into_iter()
            .map(|(x, y, progress)| {
                let (x, y) = map((x, y));
                (x, y, progress)
            })
            .collect()
    };

    let half = overlay_half_extent(settings, f64::from(size.0));
    let mut left = cursor_point.0 - half;
    let mut top = cursor_point.1 - half;
    let mut right = cursor_point.0 + half;
    let mut bottom = cursor_point.1 + half;
    for &(x, y, _) in &rings {
        left = left.min(x - half);
        top = top.min(y - half);
        right = right.max(x + half);
        bottom = bottom.max(y + half);
    }
    let x0 = left.floor().max(0.0) as u32;
    let y0 = top.floor().max(0.0) as u32;
    let x1 = (right.ceil() as i64).clamp(0, i64::from(size.0)) as u32;
    let y1 = (bottom.ceil() as i64).clamp(0, i64::from(size.1)) as u32;
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width == 0 || height == 0 {
        return;
    }

    {
        let stride = layer.stride() as usize;
        let Ok(mut data) = layer.data() else {
            return;
        };
        // The box is overwritten with the frame's own pixels, so there is no
        // stale sprite to smear: cairo then composites onto the video exactly
        // as the whole-frame path did, and only this box is ever touched.
        let frame_stride = size.0 as usize * 4;
        for y in 0..height as usize {
            let source_row = (y0 as usize + y) * stride;
            let frame_row = (y0 as usize + y) * frame_stride + x0 as usize * 4;
            for x in 0..width as usize {
                let source = &pixels[frame_row + x * 4..frame_row + x * 4 + 4];
                let target = &mut data
                    [source_row + (x0 as usize + x) * 4..source_row + (x0 as usize + x) * 4 + 4];
                target[0] = source[2];
                target[1] = source[1];
                target[2] = source[0];
                target[3] = 255;
            }
        }
    }
    layer.mark_dirty();
    {
        let Ok(context) = Context::new(&*layer) else {
            return;
        };
        context.rectangle(
            f64::from(x0),
            f64::from(y0),
            f64::from(width),
            f64::from(height),
        );
        context.clip();
        for &(x, y, progress) in &rings {
            cursor_sprite::draw_click(
                &context,
                x,
                y,
                progress,
                settings,
                frame.alpha,
                f64::from(size.0),
            );
        }
        cursor_sprite::draw(
            &context,
            cursor_point.0,
            cursor_point.1,
            1.0,
            press_cursor_scale(sidecar, source_time),
            frame.kind.as_str(),
            settings,
            frame.alpha,
        );
    }
    layer.flush();
    {
        let stride = layer.stride() as usize;
        let Ok(data) = layer.data() else {
            return;
        };
        let frame_stride = size.0 as usize * 4;
        for y in 0..height as usize {
            let source_row = (y0 as usize + y) * stride;
            let frame_row = (y0 as usize + y) * frame_stride + x0 as usize * 4;
            for x in 0..width as usize {
                let layer_x = (x0 as usize + x) * 4;
                let source = &data[source_row + layer_x..source_row + layer_x + 4];
                let target = &mut pixels[frame_row + x * 4..frame_row + x * 4 + 4];
                target[0] = source[2];
                target[1] = source[1];
                target[2] = source[0];
                target[3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        frame_plan, frame_source_time, output_size, start_export, total_frames, ExportModel,
        ExportUpdate,
    };
    use crate::recording::editor::model::CursorSettings;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn the_encoder_pipe_writes_frames_in_order_and_recycles_buffers() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);

        impl Write for Sink {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buffer);
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let sink = Sink::default();
        let pipe = super::EncoderPipe::new(sink.clone(), 4, 2);
        for index in 0..8u8 {
            let mut frame = pipe.buffer();
            assert_eq!(frame.len(), 4);
            frame.copy_from_slice(&[index; 4]);
            assert!(pipe.submit(frame).is_ok(), "the frame is queued");
        }
        assert!(pipe.finish().is_ok(), "the writer finishes cleanly");

        let expected: Vec<u8> = (0..8u8).flat_map(|index| [index; 4]).collect();
        assert_eq!(*sink.0.lock().unwrap(), expected);
    }

    #[test]
    fn the_pipe_holds_only_a_few_frames_of_a_large_export() {
        assert_eq!(super::pipe_depth(1920 * 1080 * 3 / 2), 4);
        assert_eq!(super::pipe_depth(3840 * 2160 * 3 / 2), 2);
        assert_eq!(super::pipe_depth(1), 4);
    }

    #[test]
    fn kept_spans_skip_cuts_without_gaps_or_repeats() {
        let fps = 30.0;
        let spans = frame_plan(1.0, 9.0, &[3.0, 5.0], fps);
        assert_eq!(spans.len(), 3);
        assert_eq!(total_frames(&spans), 240);
        assert_eq!(spans[0].frames, 60);
        assert_eq!(spans[1].frames, 60);
        assert_eq!(spans[2].frames, 120);
        assert!((spans[0].composition_start - 0.0).abs() < 1e-9);
        assert!((spans[1].composition_start - 2.0).abs() < 1e-9);
        assert!((spans[2].composition_start - 4.0).abs() < 1e-9);

        let last_before_cut = frame_source_time(&spans, 59, fps).unwrap();
        let first_after_cut = frame_source_time(&spans, 60, fps).unwrap();
        assert!((last_before_cut - (1.0 + 59.0 / 30.0)).abs() < 1e-9);
        assert!((first_after_cut - 3.0).abs() < 1e-9);
        assert!(last_before_cut < first_after_cut);
        assert!((frame_source_time(&spans, 120, fps).unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(frame_source_time(&spans, 240, fps), None);
    }

    #[test]
    fn a_cut_between_frames_lands_on_the_nearest_frame_boundary() {
        let fps = 30.0;
        let spans = frame_plan(0.0, 2.0, &[1.05], fps);
        assert_eq!(total_frames(&spans), 60);
        assert_eq!(spans[0].frames, 32);
        assert_eq!(spans[1].frames, 28);
        let last_kept = frame_source_time(&spans, 31, fps).unwrap();
        let first_next = frame_source_time(&spans, 32, fps).unwrap();
        assert!((first_next - last_kept - 1.0 / fps).abs() < 1e-9);
        assert!((first_next - spans[1].source_start).abs() < 1e-9);
    }

    #[test]
    fn a_trim_inside_a_frame_snaps_to_the_frame_grid() {
        let fps = 30.0;
        let spans = frame_plan(1.017, 3.017, &[], fps);
        assert_eq!(spans.len(), 1);
        assert_eq!(total_frames(&spans), 60);
        assert!((spans[0].source_start - 1.0 / 30.0 * 31.0).abs() < 1e-6);
        assert!((frame_source_time(&spans, 0, fps).unwrap() - 31.0 / 30.0).abs() < 1e-6);
    }

    #[test]
    fn ignored_cuts_and_empty_ranges_produce_no_plan() {
        assert!(frame_plan(2.0, 2.0, &[], 30.0).is_empty());
        assert!(frame_plan(0.0, 5.0, &[], 0.0).is_empty());
        assert!(frame_plan(0.0, f64::NAN, &[], 30.0).is_empty());
        let spans = frame_plan(1.0, 9.0, &[0.5, 9.5, f64::NAN], 30.0);
        assert_eq!(spans.len(), 1);
        assert_eq!(total_frames(&spans), 240);
    }

    #[test]
    fn export_size_matches_the_source_with_even_edges() {
        assert_eq!(output_size((1920, 1080)), (1920, 1080));
        assert_eq!(output_size((1919, 1079)), (1918, 1078));
        assert_eq!(output_size((0, 0)), (2, 2));
    }

    #[test]
    fn a_trimmed_cut_composition_exports_to_an_mp4() {
        if std::env::var("APEXSHOT_EXPORT_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let source = PathBuf::from(
            std::env::var("APEXSHOT_EXPORT_CLIP")
                .unwrap_or_else(|_| "C:/tmp/demo/apexshot-demo-burst.mp4".to_string()),
        );
        if !source.is_file() {
            eprintln!("skipping export test: {} is missing", source.display());
            return;
        }
        let Some(fps) = crate::recording::editor::ffmpeg::probe_frame_rate(&source) else {
            eprintln!("skipping export test: ffprobe is unavailable");
            return;
        };
        let output = std::env::temp_dir().join("apexshot-export-gated-test.mp4");
        let _ = std::fs::remove_file(&output);
        // An optional pointer sidecar exercises the sprite and the click
        // ripple in the same run; without one the pipeline still encodes.
        let sidecar = std::env::var_os("APEXSHOT_EXPORT_SIDECAR")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .and_then(|path| crate::recording::editor::sidecar::PointerSidecar::load(&path).ok());
        // `APEXSHOT_EXPORT_SPAN=0:20` times a longer run; the default span
        // sits on the demo's click burst so the cursor and ripple are exercised.
        let (trim_start, trim_end, cuts) = match std::env::var("APEXSHOT_EXPORT_SPAN")
            .ok()
            .and_then(|text| parse_span(&text))
        {
            Some((start, end)) => (start, end, Vec::new()),
            None => (9.9, 11.9, vec![10.6]),
        };
        let expected = trim_end - trim_start;
        let model = ExportModel {
            source,
            source_size: (1920, 1080),
            trim_start,
            trim_end,
            cuts,
            zooms: vec![crate::video_editor::timeline::ZoomClip {
                start: 0.0,
                end: expected,
                scale: 1.5,
                center: (960.0, 540.0),
                hidden: false,
                mode: crate::video_editor::timeline::ZoomMode::Manual,
                instant: false,
                anchor: None,
            }],
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings {
                hide_idle: false,
                ..CursorSettings::default()
            },
            sidecar,
            crf: 23,
        };
        let handle = start_export(model, output.clone());
        let mut finished = false;
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(180)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path)) => {
                    assert_eq!(path, output);
                    finished = true;
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }
        assert!(finished);
        let size = std::fs::metadata(&output).unwrap().len();
        assert!(size > 1024, "encoded file is only {size} bytes");
        let probed = probe_duration(&output).expect("ffprobe should read the export");
        assert!(
            (probed - expected).abs() <= 2.0 / fps + 1e-6,
            "duration {probed} differs from {expected}"
        );
    }

    #[test]
    fn cancelling_an_export_stops_it_and_removes_the_partial_file() {
        if std::env::var("APEXSHOT_EXPORT_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let source = PathBuf::from(
            std::env::var("APEXSHOT_EXPORT_CLIP")
                .unwrap_or_else(|_| "C:/tmp/demo/apexshot-demo-burst.mp4".to_string()),
        );
        if !source.is_file() {
            eprintln!("skipping export test: {} is missing", source.display());
            return;
        }
        let output = std::env::temp_dir().join("apexshot-export-cancel-test.mp4");
        let _ = std::fs::remove_file(&output);
        let model = ExportModel {
            source,
            source_size: (1920, 1080),
            trim_start: 0.0,
            trim_end: 60.0,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            crf: 23,
        };
        let handle = start_export(model, output.clone());
        let progress = handle
            .receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("the export should report progress");
        assert!(matches!(progress, ExportUpdate::Progress { .. }));
        handle.cancel();
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(60)) {
                Ok(ExportUpdate::Cancelled) => break,
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(other) => panic!("expected cancellation, got {:?}", kind(&other)),
                Err(error) => panic!("the export did not report back: {error}"),
            }
        }
        assert!(!output.exists(), "the partial export should be deleted");
    }

    fn kind(update: &ExportUpdate) -> &'static str {
        match update {
            ExportUpdate::Progress { .. } => "progress",
            ExportUpdate::Finished(_) => "finished",
            ExportUpdate::Failed(_) => "failed",
            ExportUpdate::Cancelled => "cancelled",
        }
    }

    #[test]
    fn the_parallel_crop_matches_the_per_pixel_reference() {
        let size = (640, 360);
        let out = (320, 180);
        let view = (10.3, 8.7, 300.5, 168.25);
        let mut pixels = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let x = (index % size.0 as usize) as u8;
            let y = (index / size.0 as usize) as u8;
            pixel.copy_from_slice(&[x, y.wrapping_mul(7), 90u8.wrapping_add(x), 255]);
        }
        let mut cropped = vec![0u8; out.0 as usize * out.1 as usize * 4];
        super::crop_scale_into(&pixels, size, view, out, &mut cropped);
        let reference = reference_crop(&pixels, size, view, out);
        assert_eq!(cropped.len(), reference.len());
        // Fixed-point weights may land one level from the float reference;
        // alpha is written opaque.
        for (index, (got, want)) in cropped.iter().zip(&reference).enumerate() {
            if index % 4 == 3 {
                assert_eq!(*got, 255, "alpha at byte {index}");
            } else {
                assert!(
                    got.abs_diff(*want) <= 1,
                    "byte {index}: crop {got}, reference {want}"
                );
            }
        }
    }

    fn reference_crop(
        pixels: &[u8],
        size: (u32, u32),
        view: (f64, f64, f64, f64),
        out: (u32, u32),
    ) -> Vec<u8> {
        let mut output = vec![0u8; out.0 as usize * out.1 as usize * 4];
        let step_x = view.2 / f64::from(out.0);
        let step_y = view.3 / f64::from(out.1);
        let width = size.0 as usize;
        let height = size.1 as usize;
        for oy in 0..out.1 as usize {
            let source_y = view.1 + (oy as f64 + 0.5) * step_y - 0.5;
            for ox in 0..out.0 as usize {
                let source_x = view.0 + (ox as f64 + 0.5) * step_x - 0.5;
                let x = source_x.clamp(0.0, width.saturating_sub(1) as f64);
                let y = source_y.clamp(0.0, height.saturating_sub(1) as f64);
                let x0 = x as usize;
                let y0 = y as usize;
                let x1 = (x0 + 1).min(width - 1);
                let y1 = (y0 + 1).min(height - 1);
                let fx = x - x0 as f64;
                let fy = y - y0 as f64;
                let offsets = [
                    (y0 * width + x0) * 4,
                    (y0 * width + x1) * 4,
                    (y1 * width + x0) * 4,
                    (y1 * width + x1) * 4,
                ];
                let offset = (oy * out.0 as usize + ox) * 4;
                for channel in 0..4 {
                    let top = f64::from(pixels[offsets[0] + channel]) * (1.0 - fx)
                        + f64::from(pixels[offsets[1] + channel]) * fx;
                    let bottom = f64::from(pixels[offsets[2] + channel]) * (1.0 - fx)
                        + f64::from(pixels[offsets[3] + channel]) * fx;
                    output[offset + channel] = (top * (1.0 - fy) + bottom * fy + 0.5) as u8;
                }
            }
        }
        output
    }

    #[test]
    fn the_boxed_cursor_overlay_matches_the_whole_frame_draw() {
        use crate::recording::editor::model::{ClickEffect, CursorSettings};
        use crate::recording::editor::sidecar::{
            CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
        };
        let size = (320, 180);
        let mut frame = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for (index, pixel) in frame.chunks_exact_mut(4).enumerate() {
            let x = (index % size.0 as usize) as u8;
            let y = (index / size.0 as usize) as u8;
            pixel.copy_from_slice(&[x, y.wrapping_mul(3), 200 - x / 2, 255]);
        }
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: size.0,
                h: size.1,
            },
        );
        sidecar.pointer = (0..30)
            .map(|step| PointerSample {
                t: step as f64 / 30.0,
                x: 180.0 + step as f64 * 2.0,
                y: 60.0 + step as f64 * 2.0,
                kind: CursorKind::Default,
            })
            .collect();
        sidecar.clicks = vec![ClickSample {
            t: 0.2,
            x: 190.0,
            y: 72.0,
            button: 1,
        }];
        let view = (100.0, 40.0, 200.0, 120.0);
        for effect in [ClickEffect::Ripple, ClickEffect::Circle] {
            let settings = CursorSettings {
                click_effect: effect,
                ..CursorSettings::default()
            };
            let mut fast = frame.clone();
            let mut layer = gtk4::cairo::ImageSurface::create(
                gtk4::cairo::Format::ARgb32,
                size.0 as i32,
                size.1 as i32,
            )
            .unwrap();
            super::draw_cursor(
                &mut fast, size, view, &sidecar, settings, 0.21, size, &mut layer,
            );
            let mut reference = frame.clone();
            legacy_draw_cursor(&mut reference, size, view, &sidecar, settings, 0.21, size);
            let max_delta = fast
                .iter()
                .zip(reference.iter())
                .map(|(left, right)| left.abs_diff(*right))
                .max()
                .unwrap_or(0);
            assert!(
                max_delta == 0,
                "{effect:?} overlay differs from the whole-frame draw by {max_delta}"
            );
        }
    }

    /// The previous exporter's compositing path: copy the whole frame into a
    /// cairo surface, draw, and copy the whole frame back. Kept in the tests as
    /// the reference the boxed overlay has to reproduce.
    fn legacy_draw_cursor(
        pixels: &mut [u8],
        size: (u32, u32),
        view: (f64, f64, f64, f64),
        sidecar: &crate::recording::editor::sidecar::PointerSidecar,
        settings: crate::recording::editor::model::CursorSettings,
        time: f64,
        source_size: (u32, u32),
    ) {
        use crate::recording::editor::model::ClickEffect;
        let motion = crate::recording::editor::sidecar::CursorMotion {
            smooth: settings.smooth,
            hide_idle: settings.hide_idle,
            idle_ms: settings.idle_ms,
            speed: settings.speed,
        };
        let Some(frame) = sidecar.presented_in_video_at(
            time,
            motion,
            f64::from(source_size.0),
            f64::from(source_size.1),
        ) else {
            return;
        };
        let mut bgra = pixels.to_vec();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut surface = gtk4::cairo::ImageSurface::create_for_data(
            bgra,
            gtk4::cairo::Format::ARgb32,
            size.0 as i32,
            size.1 as i32,
            size.0 as i32 * 4,
        )
        .unwrap();
        {
            let context = gtk4::cairo::Context::new(&surface).unwrap();
            let map = |point| {
                super::zoom::source_to_stage(point, view, f64::from(size.0), f64::from(size.1))
            };
            if settings.click_effect != ClickEffect::Ripple {
                for (x, y, progress) in sidecar.click_ripples_in_video_at(
                    time,
                    settings.click_window_seconds(),
                    f64::from(source_size.0),
                    f64::from(source_size.1),
                ) {
                    let (x, y) = map((x, y));
                    crate::recording::editor::cursor_sprite::draw_click(
                        &context,
                        x,
                        y,
                        progress,
                        settings,
                        frame.alpha,
                        f64::from(size.0),
                    );
                }
            }
            let (x, y) = map((frame.x, frame.y));
            crate::recording::editor::cursor_sprite::draw(
                &context,
                x,
                y,
                1.0,
                super::press_cursor_scale(sidecar, time),
                frame.kind.as_str(),
                settings,
                frame.alpha,
            );
        }
        surface.flush();
        let data = surface.data().unwrap();
        for (target, source) in pixels.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
            target[0] = source[2];
            target[1] = source[1];
            target[2] = source[0];
            target[3] = 255;
        }
    }

    fn parse_span(text: &str) -> Option<(f64, f64)> {
        let (start, end) = text.split_once(':')?;
        let start: f64 = start.trim().parse().ok()?;
        let end: f64 = end.trim().parse().ok()?;
        (start.is_finite() && end.is_finite() && start >= 0.0 && end > start)
            .then_some((start, end))
    }

    #[test]
    fn a_measurement_span_is_two_seconds_in_source_time() {
        assert_eq!(parse_span("0:20"), Some((0.0, 20.0)));
        assert_eq!(parse_span(" 9.9 : 11.9 "), Some((9.9, 11.9)));
        assert_eq!(parse_span("20:20"), None);
        assert_eq!(parse_span("abc"), None);
        assert_eq!(parse_span("-1:20"), None);
    }

    /// End-to-end export throughput on a generated 1080p clip, with and
    /// without a zoom, printed as ms/frame.
    /// Ignored because it shells out to ffmpeg and runs for a few seconds; run
    /// it explicitly when changing the export pipeline:
    ///
    /// ```text
    /// cargo test --release --lib -- --ignored --nocapture export_throughput
    /// ```
    #[test]
    #[ignore = "benchmark: shells out to ffmpeg"]
    fn export_throughput_benchmark() {
        use super::{start_export, ExportModel, ExportUpdate};
        use crate::recording::editor::model::CursorSettings;
        use crate::video_editor::timeline::ZoomClip;

        let dir = std::env::temp_dir().join("apexshot-export-bench");
        std::fs::create_dir_all(&dir).expect("benchmark scratch dir");
        let source = dir.join("src1080p30.mp4");
        if !source.is_file() {
            let status =
                std::process::Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
                    .args([
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-y",
                        "-f",
                        "lavfi",
                        "-i",
                        "testsrc2=size=1920x1080:rate=30",
                        "-frames:v",
                        "300",
                        "-c:v",
                        "libx264",
                        "-preset",
                        "ultrafast",
                        "-crf",
                        "16",
                    ])
                    .arg(&source)
                    .status()
                    .expect("generate benchmark clip");
            assert!(
                status.success(),
                "ffmpeg could not build the benchmark clip"
            );
        }

        let zoom = ZoomClip {
            start: 0.0,
            end: 10.0,
            scale: 1.6,
            center: (960.0, 540.0),
            hidden: false,
            mode: crate::video_editor::timeline::ZoomMode::Manual,
            instant: false,
            anchor: None,
        };
        for (label, zooms) in [("no zoom", Vec::new()), ("zoom 1.6x", vec![zoom])] {
            let output = dir.join(format!("out-{}.mp4", label.replace(' ', "-")));
            let _ = std::fs::remove_file(&output);
            let model = ExportModel {
                source: source.clone(),
                source_size: (1920, 1080),
                trim_start: 0.0,
                trim_end: 10.0,
                cuts: Vec::new(),
                zooms,
                zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
                cursor: CursorSettings::default(),
                sidecar: None,
                crf: 20,
            };
            let started = std::time::Instant::now();
            let handle = start_export(model, output.clone());
            let total = loop {
                match handle.receiver.recv_timeout(Duration::from_secs(300)) {
                    Ok(ExportUpdate::Finished(_)) => break 300.0,
                    Ok(ExportUpdate::Progress { total, .. }) => {
                        if total > 0 {
                            continue;
                        }
                    }
                    Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                    Ok(ExportUpdate::Cancelled) => panic!("export cancelled"),
                    Err(error) => panic!("export stalled: {error}"),
                }
            };
            let elapsed = started.elapsed().as_secs_f64();
            eprintln!(
                "[bench] {label}: {:.1} ms/frame ({:.0} fps) over {total:.0} frames",
                elapsed * 1000.0 / total,
                total / elapsed
            );
            let bytes = std::fs::metadata(&output).expect("output written").len();
            assert!(bytes > 1000, "{label}: output looks empty ({bytes} bytes)");
        }
    }

    fn probe_duration(path: &std::path::Path) -> Option<f64> {
        let output =
            std::process::Command::new(crate::recording::editor::ffmpeg::ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "default=noprint_wrappers=1:nokey=1",
                ])
                .arg(path)
                .output()
                .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
}

#[cfg(test)]
mod webcam_export_tests {
    use super::*;
    use crate::recording::editor::sidecar::WebcamTrack;
    use std::fs;
    use std::time::Duration;

    /// Generate a solid clip with the bundled ffmpeg, or None when there is
    /// none to run.
    fn solid_clip(path: &Path, colour: &str, size: &str, seconds: f64) -> bool {
        let status = Command::new(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", &format!("color=c={colour}:s={size}:d={seconds}:r=30")])
            .args(["-pix_fmt", "yuv420p"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        matches!(status, Ok(status) if status.success())
    }

    /// One frame of the file as RGB rows, for pixel checks.
    fn frame_rgb(path: &Path, seconds: f64, size: (u32, u32)) -> Option<Vec<u8>> {
        let output = Command::new(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-ss", &format!("{seconds:.3}")])
            .arg("-i")
            .arg(path)
            .args(["-frames:v", "1"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
            .output()
            .ok()?;
        let expected = size.0 as usize * size.1 as usize * 3;
        (output.stdout.len() >= expected).then(|| output.stdout[..expected].to_vec())
    }

    fn pixel(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let offset = (y as usize * width as usize + x as usize) * 3;
        [frame[offset], frame[offset + 1], frame[offset + 2]]
    }

    #[test]
    fn a_webcam_track_is_composited_into_the_exported_frames() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-webcam-export-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let source = dir.join("screen.mp4");
        if !solid_clip(&source, "0x1e1e1e", "320x240", 2.0) {
            eprintln!("skipping the webcam export test: ffmpeg is unavailable");
            return;
        }
        let track_path = webcam_track_file(&source);
        let Some(track_path) = track_path else {
            eprintln!("skipping the webcam export test: no sidecar directory");
            return;
        };
        if let Some(parent) = track_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !solid_clip(&track_path, "0x00ff00", "320x320", 2.0) {
            return;
        }

        let track = WebcamTrack {
            file: track_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            width: 320,
            height: 320,
            duration: 2.0,
            x: 0.6,
            y: 0.6,
            w: 0.35,
            h: 0.35,
            shape: 1,
            flip: false,
        };
        let mut sidecar = PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion::from_capture(None, None, None, None),
        );
        sidecar.webcam = Some(track);
        let sidecar_path = PointerSidecar::sidecar_path(&source);
        if sidecar.write(&sidecar_path).is_err() {
            return;
        }

        let output = dir.join("export.mp4");
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 1.5,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: Some(sidecar),
            crf: 30,
        };
        let handle = start_export(model, output.clone());
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(120)) {
                Ok(ExportUpdate::Progress { .. }) => {}
                Ok(ExportUpdate::Finished(path)) => {
                    assert_eq!(path, output);
                    break;
                }
                Ok(ExportUpdate::Failed(message)) => panic!("export failed: {message}"),
                Ok(ExportUpdate::Cancelled) => panic!("export cancelled itself"),
                Err(error) => panic!("export did not report back: {error}"),
            }
        }

        let frame = frame_rgb(&output, 0.5, (320, 240)).expect("the export should decode");
        // The bubble covers 0.6..0.95 of the frame; its middle is the camera.
        let inside = pixel(&frame, 320, 240, 180);
        assert!(
            inside[1] > 150 && inside[0] < 90,
            "the camera should be composited at the bubble: {inside:?}"
        );
        // The footage outside it is still the recording.
        let outside = pixel(&frame, 320, 20, 20);
        assert!(
            outside[0] < 70 && outside[2] < 70,
            "the footage should be untouched outside the bubble: {outside:?}"
        );

        let _ = fs::remove_file(&output);
        let _ = fs::remove_file(&sidecar_path);
        let _ = fs::remove_file(&track_path);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The track lands beside the pointer sidecar, exactly as the recorder
    /// writes it.
    fn webcam_track_file(source: &Path) -> Option<PathBuf> {
        PointerSidecar::sidecar_path(source)
            .parent()
            .map(|dir| dir.join("apexshot-webcam-export-test.mp4"))
    }
}
