//! Export the composition to an H.264 MP4.
//!
//! The frame plan walks the kept source spans (trim minus cuts) in composition
//! order, and every frame is composited by the same code the preview uses:
//! the follow camera, the click ripple warp, and the cursor sprite with its
//! press spring. The canvas around the video — the background fill, padding,
//! frame aspect, and rounded corners — is laid on afterwards by
//! `canvas::FramedSink`, so every render path produces only the video rect.
//! Audio is not part of this path.

mod canvas;
#[cfg(target_os = "windows")]
mod gpu;
mod hybrid;

pub(super) use canvas::CanvasLayout;

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
use crate::recording::editor::sidecar::{CursorMotion, PointerSidecar};
use gtk4::cairo::{Context, Format, ImageSurface};
use rayon::prelude::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_FPS: f64 = 30.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct ExportSettings {
    /// None preserves the recording's frame rate.
    pub fps: Option<u32>,
    /// The short edge, preserving the recording's aspect ratio. None keeps its size.
    pub resolution: Option<u32>,
    /// Use the GPU encoder when the machine has one that opens.
    pub hardware: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            fps: None,
            resolution: None,
            hardware: true,
        }
    }
}

impl ExportSettings {
    pub fn dimensions(self, source: (u32, u32)) -> (u32, u32) {
        let Some(edge) = self.resolution else {
            return output_size(source);
        };
        let ratio = f64::from(edge) / f64::from(source.0.min(source.1).max(1));
        output_size((
            (f64::from(source.0) * ratio).round() as u32,
            (f64::from(source.1) * ratio).round() as u32,
        ))
    }
}

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
    /// The camera layer to bake, when the recording has one.
    pub webcam: Option<webcam::WebcamExport>,
    /// The canvas around the video (fill, padding, frame, rounded corners), or
    /// None to export the video edge to edge.
    pub canvas: Option<CanvasLayout>,
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

#[cfg(test)]
pub(super) fn start_export(model: ExportModel, output: PathBuf) -> ExportHandle {
    start_export_with_settings(model, output, ExportSettings::default())
}

pub(super) fn start_export_with_settings(
    mut model: ExportModel,
    output: PathBuf,
    settings: ExportSettings,
) -> ExportHandle {
    let cuts = model.cuts.clone();
    if let Some(sidecar) = model.sidecar.as_mut() {
        sidecar.set_cut_times(&cuts);
    }
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    if let Err(error) =
        crate::studio_entitlement::ensure_export_allowed(super::studio::capabilities(&model))
    {
        let _ = sender.send(ExportUpdate::Failed(error.to_string()));
        return ExportHandle { receiver, cancel };
    }
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        let encoder = Encoder::fastest();
        let mut result = render(&model, &output, &sender, &thread_cancel, encoder, settings);
        if encoder != Encoder::X264 {
            if let Err(End::Failed(message)) = &result {
                eprintln!(
                    "[export] {} failed ({message}); retrying with libx264",
                    encoder.codec()
                );
                result = render(
                    &model,
                    &output,
                    &sender,
                    &thread_cancel,
                    Encoder::X264,
                    settings,
                );
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
            [Self::Nvenc, Self::QuickSync, Self::Amf]
                .into_iter()
                .find(|encoder| encoder.opens())
                .unwrap_or(Self::X264)
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
    fn args(self, crf: u32, size: (u32, u32), fps: f64) -> Vec<String> {
        let crf = crf.to_string();
        let crf = crf.as_str();
        let profile = if self == Self::X264 && crf == "0" {
            "high444"
        } else {
            "high"
        };
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
        [
            "-c:v",
            self.codec(),
            "-profile:v",
            profile,
            "-level:v",
            h264_level(size, fps),
        ]
        .into_iter()
        .chain(settings)
        .map(str::to_string)
        .collect()
    }

    /// Whether ffmpeg can open this encoder here: a build lists every GPU
    /// encoder it was compiled with, whether or not the machine has the
    /// hardware or a new enough driver for it.
    fn opens(self) -> bool {
        crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-f", "lavfi", "-i", "color=c=black:s=320x240:r=30"])
            .args(["-frames:v", "2"])
            .args(self.args(23, (320, 240), 30.0))
            .args(["-f", "null", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[derive(Debug)]
enum End {
    Cancelled,
    Failed(String),
}

struct RenderPlan {
    fps: f64,
    spans: Vec<SourceSpan>,
    total: u64,
    mapping: TimeMapping,
    /// The video rect the render paths produce.
    size: (u32, u32),
    /// The encoded frame: the canvas when the export has one, otherwise `size`.
    output: (u32, u32),
    /// The decoded NV12 can go to the encoder unchanged: no crop, cursor,
    /// ripple, camera layer, or resize touches the frame.
    passthrough: bool,
}

fn plan_render(model: &ExportModel, settings: ExportSettings) -> Result<RenderPlan, End> {
    let fps = settings
        .fps
        .map(f64::from)
        .or_else(|| ffmpeg::probe_frame_rate(&model.source))
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
    let output = settings.dimensions(
        model
            .canvas
            .as_ref()
            .map_or(model.source_size, |canvas| canvas.size),
    );
    let size = model
        .canvas
        .as_ref()
        .map_or(output, |canvas| canvas::video_size(canvas, output));
    let passthrough = size == model.source_size
        && model.sidecar.is_none()
        && model.webcam.is_none()
        && model.zooms.iter().all(|zoom| zoom.hidden);
    Ok(RenderPlan {
        fps,
        spans,
        total,
        mapping,
        size,
        output,
        passthrough,
    })
}

/// The chunked two-encoder path is opt-in until the compositor stops being
/// the bottleneck; at that point encoding is split, not the frame loop.
fn hybrid_requested() -> bool {
    std::env::var("APEXSHOT_EXPORT_HYBRID").ok().as_deref() == Some("1")
}

/// A recovery switch for machines whose hardware encoder misbehaves.
fn software_forced() -> bool {
    std::env::var("APEXSHOT_EXPORT_HARDWARE").ok().as_deref() == Some("0")
}

fn worker_override() -> Option<usize> {
    std::env::var("APEXSHOT_EXPORT_WORKERS")
        .ok()?
        .parse::<usize>()
        .ok()
}

fn render(
    model: &ExportModel,
    output: &Path,
    sender: &Sender<ExportUpdate>,
    cancel: &Arc<AtomicBool>,
    encoder: Encoder,
    settings: ExportSettings,
) -> Result<(), End> {
    render_with_workers(model, output, sender, cancel, encoder, settings, None)
}

fn render_with_workers(
    model: &ExportModel,
    output: &Path,
    sender: &Sender<ExportUpdate>,
    cancel: &Arc<AtomicBool>,
    encoder: Encoder,
    settings: ExportSettings,
    workers: Option<usize>,
) -> Result<(), End> {
    let plan = plan_render(model, settings)?;
    let hardware = settings.hardware && !software_forced();
    let primary = if hardware { encoder } else { Encoder::X264 };
    let workers = workers
        .or_else(worker_override)
        .unwrap_or_else(|| worker_count(&plan, primary))
        .clamp(1, 6);
    let done = Arc::new(AtomicU64::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    let reporter = {
        let done = done.clone();
        let finished = finished.clone();
        let sender = sender.clone();
        let total = plan.total;
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                let _ = sender.send(ExportUpdate::Progress {
                    done: done.load(Ordering::Relaxed),
                    total,
                });
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let result = if workers > 1 {
        render_parallel(
            model, output, cancel, primary, settings, &plan, &done, workers,
        )
    } else {
        render_range(
            model,
            output,
            cancel,
            primary,
            settings,
            &plan,
            &done,
            (0, plan.total),
            true,
            hybrid_requested() && hardware,
        )
    };
    finished.store(true, Ordering::Relaxed);
    let _ = reporter.join();
    result
}

/// Compositor frames an export range. Every range opens its own decoder,
/// compositor, and encoder, so ranges run on separate cores and join later.
#[allow(clippy::too_many_arguments)]
fn render_range(
    model: &ExportModel,
    output: &Path,
    cancel: &AtomicBool,
    encoder: Encoder,
    settings: ExportSettings,
    plan: &RenderPlan,
    done: &AtomicU64,
    range: (u64, u64),
    faststart: bool,
    allow_hybrid: bool,
) -> Result<(), End> {
    let (first, end) = range;
    if first >= end {
        return Ok(());
    }
    let fps = plan.fps;
    let size = plan.size;
    let source = (
        f64::from(model.source_size.0),
        f64::from(model.source_size.1),
    );

    let probed = ffmpeg::probe_color_space(&model.source);
    let space = YuvSpace::from_probe(probed.0.as_deref(), probed.1.as_deref());
    let converter = Nv12Converter::new(space);
    let sink = Sink::open(
        output,
        plan.output,
        fps,
        model.crf,
        encoder,
        plan.total,
        allow_hybrid && settings.hardware && encoder != Encoder::X264,
        faststart,
    )?;
    let framer = match &model.canvas {
        Some(layout) => Some(canvas::Framer::new(layout, plan.output, &converter)?),
        None => None,
    };
    let mut sink = canvas::FramedSink::new(sink, framer);
    let mut camera = ZoomCameraCache::default();
    let camera_settings = zoom::ZoomCameraSettings {
        frame_rate: fps,
        ..model.zoom_camera
    };
    let mut decoded = if plan.passthrough {
        Vec::new()
    } else {
        vec![0u8; model.source_size.0 as usize * model.source_size.1 as usize * 4]
    };
    let mut packed = vec![0u8; model.source_size.0 as usize * model.source_size.1 as usize * 3 / 2];
    let mut scaled = if plan.passthrough {
        Vec::new()
    } else {
        vec![0u8; size.0 as usize * size.1 as usize * 4]
    };
    let mut cursor_layer = match &model.sidecar {
        Some(_) => Some(
            ImageSurface::create(Format::ARgb32, size.0 as i32, size.1 as i32).map_err(
                |error| End::Failed(format!("could not allocate the cursor layer: {error}")),
            )?,
        ),
        None => None,
    };
    let webcam = model.webcam.clone();
    let layouts = webcam
        .as_ref()
        .map(|webcam| webcam.layouts.clone().clamped())
        .unwrap_or_default();
    let source_aspect = f64::from(model.source_size.0) / f64::from(model.source_size.1);
    let camera_aspect = webcam
        .as_ref()
        .filter(|webcam| webcam.source_size.0 > 0 && webcam.source_size.1 > 0)
        .map(|webcam| f64::from(webcam.source_size.0) / f64::from(webcam.source_size.1))
        .unwrap_or(16.0 / 9.0);
    #[cfg(target_os = "windows")]
    let mut gpu = if !plan.passthrough && model.sidecar.is_none() && webcam.is_none() {
        match gpu::Scaler::new(model.source_size, size, space, fps) {
            Ok(scaler) => {
                eprintln!(
                    "[export] GPU crop and scale: {}x{} -> {}x{}",
                    model.source_size.0, model.source_size.1, size.0, size.1
                );
                Some(scaler)
            }
            Err(error) => {
                eprintln!("[export] GPU crop and scale unavailable: {error}");
                None
            }
        }
    } else {
        None
    };
    let mut cutout = if layouts.uses_cutout() {
        match super::cutout::CutoutProcessor::new() {
            Ok(processor) => Some(processor),
            Err(error) => {
                return Err(End::Failed(format!(
                    "the camera cutout could not start: {error}"
                )));
            }
        }
    } else {
        None
    };
    let mut done_index = first;
    let started = Instant::now();
    let log_frame = |done_index: u64| {
        if done_index.is_multiple_of(100) || done_index == end {
            let elapsed = started.elapsed().as_secs_f64();
            eprintln!(
                "[export] frame {done_index}/{} at {:.1} ms/frame",
                plan.total,
                elapsed * 1000.0 / (done_index - first + 1) as f64
            );
        }
    };
    let mut span_base = 0u64;

    for span in &plan.spans {
        let span_end = span_base + span.frames;
        if span_end <= first {
            span_base = span_end;
            continue;
        }
        if span_base >= end {
            break;
        }
        let local_first = first.saturating_sub(span_base);
        let local_last = (end - span_base).min(span.frames);
        let count = local_last.saturating_sub(local_first);
        if count == 0 {
            span_base = span_end;
            continue;
        }
        let (mut decoder, mut stdout) = match spawn_decoder(
            &model.source,
            *span,
            local_first,
            count,
            fps,
            model.source_size,
        ) {
            Ok(mut decoder) => match decoder.stdout.take() {
                Some(stdout) => (decoder, stdout),
                None => {
                    return Err(End::Failed(
                        "the decoder did not open its output".to_string(),
                    ));
                }
            },
            Err(error) => {
                return Err(error);
            }
        };
        let mut track_reader = match webcam.as_ref() {
            Some(webcam) => {
                let at = span.source_start + local_first as f64 / fps;
                let start = webcam.start_at(at);
                let lead = webcam.lead_at(at);
                let duration = count as f64 / fps;
                let decode = webcam::camera_decode_size(webcam.source_size, size);
                match webcam::TrackReader::open(
                    &webcam.path,
                    start,
                    duration,
                    fps,
                    decode,
                    webcam.time_scale(),
                    lead,
                ) {
                    Some(reader) => Some(reader),
                    None => {
                        let _ = decoder.kill();
                        let _ = decoder.wait();
                        return Err(End::Failed(format!(
                            "could not open the camera track: {}",
                            webcam.path.display()
                        )));
                    }
                }
            }
            None => None,
        };
        let mut span_frame = 0u64;
        while span_frame < count {
            if cancel.load(Ordering::Relaxed) {
                let _ = decoder.kill();
                let _ = decoder.wait();
                return Err(End::Cancelled);
            }
            if stdout.read_exact(&mut packed).is_err() {
                break;
            }
            span_frame += 1;
            if plan.passthrough {
                let mut nv12 = match sink.buffer(done_index) {
                    Ok(buffer) => buffer,
                    Err(error) => {
                        let _ = decoder.kill();
                        let _ = decoder.wait();
                        return Err(error);
                    }
                };
                std::mem::swap(&mut packed, &mut nv12);
                if let Err(error) = sink.submit(nv12) {
                    let _ = decoder.kill();
                    let _ = decoder.wait();
                    return Err(error);
                }
                done_index += 1;
                done.fetch_add(1, Ordering::Relaxed);
                log_frame(done_index);
                continue;
            }
            let source_time = frame_source_time(&plan.spans, done_index, fps)
                .ok_or_else(|| End::Failed("the frame plan ran out of frames".to_string()))?;
            let composition_time = done_index as f64 / fps;
            let camera_value = camera.evaluate(
                &model.zooms,
                source,
                &plan.mapping,
                camera_settings,
                composition_time,
                || model.sidecar.clone(),
            );
            let view = zoom::view_rect(camera_value.0, camera_value.1, source);
            let trail = if zoom::instant_blur_disabled(&model.zooms, composition_time) {
                None
            } else {
                let previous = camera.evaluate(
                    &model.zooms,
                    source,
                    &plan.mapping,
                    camera_settings,
                    (composition_time - 1.0 / fps).max(0.0),
                    || model.sidecar.clone(),
                );
                super::motion_blur::ScreenTrail::new(
                    view,
                    zoom::view_rect(previous.0, previous.1, source),
                    source,
                    size,
                    fps,
                    model.zoom_camera.motion_blur,
                )
            };
            let cropped =
                trail.is_some() || !(view_is_full(view, source) && size == model.source_size);
            // The GPU path only crops and scales; a blurred frame takes the CPU path.
            #[cfg(target_os = "windows")]
            if trail.is_none() {
                if let Some(scaler) = gpu.as_mut() {
                    let mut nv12 = match sink.buffer(done_index) {
                        Ok(buffer) => buffer,
                        Err(error) => {
                            let _ = decoder.kill();
                            let _ = decoder.wait();
                            return Err(error);
                        }
                    };
                    let rendered = if cropped {
                        scaler.scale(&packed, view, &mut nv12)
                    } else {
                        std::mem::swap(&mut packed, &mut nv12);
                        Ok(())
                    };
                    match rendered {
                        Ok(()) => {
                            if let Err(error) = sink.submit(nv12) {
                                let _ = decoder.kill();
                                let _ = decoder.wait();
                                return Err(error);
                            }
                            done_index += 1;
                            done.fetch_add(1, Ordering::Relaxed);
                            log_frame(done_index);
                            continue;
                        }
                        Err(error) => {
                            eprintln!(
                                "[export] GPU crop and scale failed: {error}; continuing on the CPU"
                            );
                            gpu = None;
                        }
                    }
                }
            }
            converter.to_rgba_into(&packed, model.source_size, &mut decoded);
            if let Some(trail) = trail {
                trail.render(&decoded, model.source_size, size, &mut scaled, 16, 4.0, 4);
            } else if cropped {
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
                    model.source.as_path(),
                    model.cursor,
                    source_time,
                    model.source_size,
                    layer,
                );
            }
            // The camera layer sits over the cursor, the way the bubble window
            // sat over the desktop while the recording ran.
            let camera_frame = track_reader.as_mut().and_then(|reader| reader.next_frame());
            if let Some(webcam) = webcam.as_ref() {
                let shrink = webcam::zoom_shrink_at(
                    &model.zooms,
                    composition_time,
                    model.zoom_camera.lead_in_seconds(),
                );
                let layout = layouts.frame_at_zoomed(
                    composition_time,
                    webcam.settings,
                    source_aspect,
                    camera_aspect,
                    shrink,
                );
                if layout.camera_alpha > 0.0 && camera_frame.is_none() {
                    let _ = decoder.kill();
                    let _ = decoder.wait();
                    return Err(End::Failed(format!(
                        "the camera track could not be decoded: {}",
                        webcam.path.display()
                    )));
                }
                let mask = if layout.cutout > 1e-3 && layout.camera_alpha > 0.0 {
                    let (pixels, matte_size) = camera_frame.expect("checked above");
                    let Some(processor) = cutout.as_mut() else {
                        let _ = decoder.kill();
                        let _ = decoder.wait();
                        return Err(End::Failed(
                            "the camera cutout has no segmentation model".to_string(),
                        ));
                    };
                    match processor.matte(pixels, matte_size) {
                        Ok(mask) => Some(mask),
                        Err(error) => {
                            let _ = decoder.kill();
                            let _ = decoder.wait();
                            return Err(End::Failed(format!("the camera cutout failed: {error}")));
                        }
                    }
                } else {
                    None
                };
                let (camera_pixels, camera_size) = match camera_frame {
                    Some((pixels, size)) => (pixels, size),
                    None => (&[][..], (0, 0)),
                };
                if let Err(error) = webcam::composite_layout_into(
                    frame,
                    size,
                    layout,
                    camera_pixels,
                    camera_size,
                    mask.as_deref(),
                ) {
                    let _ = decoder.kill();
                    let _ = decoder.wait();
                    return Err(End::Failed(error));
                }
            }

            let mut nv12 = match sink.buffer(done_index) {
                Ok(buffer) => buffer,
                Err(error) => {
                    let _ = decoder.kill();
                    let _ = decoder.wait();
                    return Err(error);
                }
            };
            converter.convert_into(frame, size, &mut nv12);
            if let Err(error) = sink.submit(nv12) {
                let _ = decoder.kill();
                let _ = decoder.wait();
                return Err(error);
            }
            done_index += 1;
            done.fetch_add(1, Ordering::Relaxed);
            log_frame(done_index);
        }
        let _ = decoder.kill();
        let _ = decoder.wait();
        if span_frame < count {
            return Err(End::Failed(
                "the recording ended before the planned frames".to_string(),
            ));
        }
        span_base = span_end;
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(End::Cancelled);
    }
    sink.finish()?;
    Ok(())
}

fn worker_count(plan: &RenderPlan, primary: Encoder) -> usize {
    if plan.passthrough || plan.total < (plan.fps * 8.0).round() as u64 {
        return 1;
    }
    let cores = std::thread::available_parallelism().map_or(1, |count| count.get());
    let by_cores = cores / 3;
    let pixels = u64::from(plan.output.0) * u64::from(plan.output.1);
    let by_resolution = if pixels >= 3840 * 2160 {
        2
    } else if pixels >= 2560 * 1440 {
        3
    } else {
        4
    };
    let by_sessions = if primary == Encoder::X264 { 4 } else { 3 };
    let by_frames = (plan.total / (plan.fps * 4.0).round().max(1.0) as u64).max(1) as usize;
    by_cores
        .clamp(1, 6)
        .min(by_resolution)
        .min(by_sessions)
        .min(by_frames)
}

fn split_ranges(total: u64, workers: usize) -> Vec<(u64, u64)> {
    let workers = workers.max(1) as u64;
    let base = total / workers;
    let extra = total % workers;
    let mut start = 0u64;
    (0..workers)
        .filter_map(|index| {
            let length = base + u64::from(index < extra);
            let range = (start, start + length);
            start += length;
            (range.1 > range.0).then_some(range)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_parallel(
    model: &ExportModel,
    output: &Path,
    cancel: &Arc<AtomicBool>,
    encoder: Encoder,
    settings: ExportSettings,
    plan: &RenderPlan,
    done: &AtomicU64,
    workers: usize,
) -> Result<(), End> {
    let dir = hybrid::PartDir::create(output)?;
    let ranges = split_ranges(plan.total, workers);
    let mut parts = Vec::with_capacity(ranges.len());
    let mut failure = None;
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(ranges.len());
        for (index, range) in ranges.iter().copied().enumerate() {
            let part = dir.path().join(format!("part-{index:03}.mp4"));
            parts.push(part.clone());
            handles.push(scope.spawn(move || {
                render_range(
                    model, &part, cancel, encoder, settings, plan, done, range, false, false,
                )
                .map_err(|error| (index, error))
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err((index, error))) => {
                    if failure.is_none() {
                        failure = Some((index, error));
                    }
                    cancel.store(true, Ordering::Relaxed);
                }
                Err(_) => {
                    if failure.is_none() {
                        failure = Some((
                            usize::MAX,
                            End::Failed("an export worker panicked".to_string()),
                        ));
                    }
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }
    });
    if let Some((_, error)) = failure {
        return Err(error);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(End::Cancelled);
    }
    hybrid::join_segments(dir.path(), &parts, output)
}

/// Where composited frames go: one encoder for the whole file, or the chunked
/// pair that lets a hardware encoder and the CPU encoder share the timeline.
enum Sink {
    Single(SingleSink),
    Segmented(Box<hybrid::SegmentedSink>),
}

impl Sink {
    #[allow(clippy::too_many_arguments)]
    fn open(
        output: &Path,
        size: (u32, u32),
        fps: f64,
        crf: u32,
        primary: Encoder,
        total: u64,
        hybrid: bool,
        faststart: bool,
    ) -> Result<Self, End> {
        if hybrid && hybrid::supported(primary, total, fps) {
            let sink = hybrid::SegmentedSink::new(primary, output, size, fps, crf, total)?;
            eprintln!(
                "[export] encoder: {} + libx264 (chunked pieces)",
                primary.codec()
            );
            return Ok(Self::Segmented(Box::new(sink)));
        }
        eprintln!("[export] encoder: {}", primary.codec());
        let mut child = spawn_encoder(output, size, fps, crf, primary, faststart)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| End::Failed("the encoder did not open its input".to_string()))?;
        let frame_bytes = size.0 as usize * size.1 as usize * 3 / 2;
        Ok(Self::Single(SingleSink {
            pipe: Some(EncoderPipe::new(
                stdin,
                frame_bytes,
                pipe_depth(frame_bytes),
            )),
            child: Some(child),
        }))
    }

    fn buffer(&mut self, index: u64) -> Result<Vec<u8>, End> {
        match self {
            Self::Single(sink) => sink.buffer(),
            Self::Segmented(sink) => sink.buffer(index),
        }
    }

    fn submit(&mut self, frame: Vec<u8>) -> Result<(), End> {
        match self {
            Self::Single(sink) => sink.submit(frame),
            Self::Segmented(sink) => sink.submit(frame),
        }
    }

    fn finish(self) -> Result<(), End> {
        match self {
            Self::Single(sink) => sink.finish(),
            Self::Segmented(sink) => sink.finish(),
        }
    }
}

struct SingleSink {
    pipe: Option<EncoderPipe>,
    child: Option<Child>,
}

impl SingleSink {
    fn buffer(&self) -> Result<Vec<u8>, End> {
        self.pipe
            .as_ref()
            .map(EncoderPipe::buffer)
            .ok_or_else(|| End::Failed("the encoder stopped accepting frames".to_string()))
    }

    fn submit(&self, frame: Vec<u8>) -> Result<(), End> {
        self.pipe
            .as_ref()
            .ok_or_else(|| End::Failed("the encoder stopped accepting frames".to_string()))?
            .submit(frame)
    }

    fn finish(mut self) -> Result<(), End> {
        let pipe = self
            .pipe
            .take()
            .ok_or_else(|| End::Failed("the encoder stopped accepting frames".to_string()))?;
        let mut child = self
            .child
            .take()
            .ok_or_else(|| End::Failed("the encoder disappeared".to_string()))?;
        if let Err(error) = pipe.finish() {
            stop(&mut child);
            return Err(error);
        }
        let result = child
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
}

impl Drop for SingleSink {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            stop(&mut child);
        }
        self.pipe.take();
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_decoder(
    source: &Path,
    span: SourceSpan,
    first: u64,
    count: u64,
    fps: f64,
    size: (u32, u32),
) -> Result<Child, End> {
    let source_start = span.source_start + first as f64 / fps;
    let duration = count as f64 / fps;
    crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-ss", &format!("{source_start:.6}")])
        .arg("-i")
        .arg(source)
        .args(["-t", &format!("{duration:.6}")])
        .arg("-an")
        // A seek between source frames leaves a positive first timestamp.
        // Fill that leading gap on the output grid instead of losing a frame.
        .args(["-vf", &format!("scale={}:{},fps={fps}:start_time=0", size.0, size.1)])
        .args(["-frames:v", &count.to_string()])
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
    faststart: bool,
) -> Result<Child, End> {
    let mut command = crate::utils::hidden_command(ffmpeg::ffmpeg_executable());
    command
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
        .args(encoder.args(crf, size, fps));
    if faststart {
        command.args(["-movflags", "+faststart"]);
    }
    command
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| End::Failed(format!("could not start the encoder: {error}")))
}

/// The smallest H.264 level that holds the frame size and rate.
///
/// Every encoder in a chunked export is opened with this level, so the pieces
/// share a stream format and the join can copy packets instead of re-encoding.
/// The table is the level's macroblock and macroblock-rate limit.
fn h264_level(size: (u32, u32), fps: f64) -> &'static str {
    const LEVELS: [(&str, u64, u64); 8] = [
        ("3.1", 3_600, 108_000),
        ("3.2", 5_120, 216_000),
        ("4.0", 8_192, 245_760),
        ("4.1", 8_192, 245_760),
        ("4.2", 8_704, 522_240),
        ("5.0", 22_080, 589_824),
        ("5.1", 36_864, 983_040),
        ("5.2", 36_864, 2_073_600),
    ];
    let macroblocks = u64::from(size.0.div_ceil(16)) * u64::from(size.1.div_ceil(16));
    let rate = macroblocks as f64 * fps.max(1.0);
    LEVELS
        .iter()
        .find(|(_, frame, throughput)| macroblocks <= *frame && rate <= *throughput as f64)
        .map(|(name, _, _)| *name)
        .unwrap_or("5.2")
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

    /// Stop accepting frames without waiting for the encoder to finish. The
    /// writer drains what is already queued while the next piece starts.
    fn close(&mut self) {
        self.queue.take();
    }

    fn is_finished(&self) -> bool {
        self.writer
            .as_ref()
            .is_none_or(|writer| writer.is_finished())
    }

    /// Close the pipe and wait for the writer, reporting the first write
    /// error the encoder caused.
    fn finish(mut self) -> Result<(), End> {
        self.close();
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
#[allow(clippy::too_many_arguments)]
fn draw_cursor(
    pixels: &mut [u8],
    size: (u32, u32),
    view: ViewRect,
    sidecar: &PointerSidecar,
    source_path: &Path,
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
        rotation_mode: settings.rotation_mode,
        rotation_ratio: settings.rotate_on_x_movement_ratio,
        base_rotation_deg: settings.base_rotation_deg,
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
        let capture = sidecar.cursor_capture(source_path, &frame);
        cursor_sprite::draw_recorded(
            &context,
            cursor_point.0,
            cursor_point.1,
            1.0,
            press_cursor_scale(sidecar, source_time),
            &frame,
            capture.as_ref(),
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
        frame_plan, frame_source_time, h264_level, output_size, render, render_with_workers,
        split_ranges, start_export, total_frames, zoom, Encoder, End, ExportModel, ExportSettings,
        ExportUpdate,
    };
    use crate::recording::editor::ffmpeg;
    use crate::recording::editor::model::{CursorRotationMode, CursorSettings};
    use std::path::PathBuf;
    use std::sync::{atomic::AtomicBool, mpsc, Arc};
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
    fn resolution_presets_keep_aspect_ratio_and_even_edges() {
        let settings = ExportSettings {
            resolution: Some(720),
            ..ExportSettings::default()
        };
        assert_eq!(settings.dimensions((1920, 1080)), (1280, 720));
        assert_eq!(settings.dimensions((1080, 1920)), (720, 1280));
        assert_eq!(settings.dimensions((1440, 1080)), (960, 720));
        assert_eq!(
            ExportSettings::default().dimensions((1919, 1079)),
            (1918, 1078)
        );
    }

    #[test]
    fn h264_levels_cover_common_export_sizes() {
        assert_eq!(h264_level((1280, 720), 30.0), "3.1");
        assert_eq!(h264_level((1920, 1080), 30.0), "4.0");
        assert_eq!(h264_level((1920, 1080), 60.0), "4.2");
        assert_eq!(h264_level((2560, 1440), 30.0), "5.0");
        assert_eq!(h264_level((3840, 2160), 30.0), "5.1");
        assert_eq!(h264_level((3840, 2160), 60.0), "5.2");
    }

    #[test]
    fn parallel_ranges_cover_each_frame_once() {
        for (total, workers) in [(300, 2), (301, 4), (5, 6), (1, 4)] {
            let ranges = split_ranges(total, workers);
            let covered: u64 = ranges.iter().map(|(first, end)| end - first).sum();
            assert_eq!(covered, total);
            assert_eq!(ranges.first().unwrap().0, 0);
            assert_eq!(ranges.last().unwrap().1, total);
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].1, pair[1].0);
            }
        }
    }

    #[test]
    #[ignore = "integration: shells out to ffmpeg"]
    fn parallel_ranges_match_the_sequential_render() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-parallel-parity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=640x360:r=30:d=6",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("ffmpeg should be available");
        assert!(generated.success());
        let model = ExportModel {
            source,
            source_size: (640, 360),
            trim_start: 0.0,
            trim_end: 6.0,
            cuts: vec![2.0],
            zooms: vec![crate::video_editor::timeline::ZoomClip {
                start: 1.0,
                end: 4.0,
                scale: 1.5,
                center: (320.0, 180.0),
                hidden: false,
                mode: crate::video_editor::timeline::ZoomMode::Manual,
                instant: false,
                anchor: None,
            }],
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 0,
        };
        let settings = ExportSettings {
            fps: Some(30),
            hardware: false,
            ..ExportSettings::default()
        };
        let mut outputs = Vec::new();
        for (workers, name) in [(1usize, "sequential"), (2, "parallel")] {
            let output = dir.join(format!("{name}.mp4"));
            let (sender, _receiver) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            render_with_workers(
                &model,
                &output,
                &sender,
                &cancel,
                Encoder::X264,
                settings,
                Some(workers),
            )
            .unwrap_or_else(|error| panic!("{name} failed: {error:?}"));
            outputs.push(output);
        }
        let compare = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "info", "-i"])
            .arg(&outputs[0])
            .arg("-i")
            .arg(&outputs[1])
            .args(["-lavfi", "[0:v][1:v]psnr", "-f", "null", "-"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&compare.stderr);
        assert!(compare.status.success(), "{text}");
        // The CPU compositor is bit-exact across a split. The D3D11 video
        // processor can pick a different filter when a range starts mid-zoom,
        // which moves a few levels; the frames still line up, so accept a high
        // PSNR instead of `inf` for that path.
        let average = text
            .split("average:")
            .nth(1)
            .and_then(|tail| tail.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok());
        assert!(
            text.contains("average:inf") || average.is_some_and(|value| value >= 45.0),
            "parallel output differs: {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn motion_blur_reaches_real_export_frames() {
        if std::env::var("APEXSHOT_EXPORT_TEST").ok().as_deref() != Some("1") {
            return;
        }
        use crate::video_editor::timeline::{ZoomClip, ZoomMode};
        let dir =
            std::env::temp_dir().join(format!("apexshot-blur-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let status = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x180:rate=60:duration=2",
                "-threads",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        let mut model = ExportModel {
            source,
            source_size: (320, 180),
            trim_start: 0.0,
            trim_end: 2.0,
            cuts: vec![],
            zooms: vec![ZoomClip {
                start: 0.2,
                end: 1.4,
                scale: 2.0,
                center: (220.0, 110.0),
                hidden: false,
                mode: ZoomMode::Manual,
                instant: false,
                anchor: None,
            }],
            zoom_camera: zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 18,
        };
        for fps in [24, 60] {
            let mut videos = Vec::new();
            for amount in [0.0, 1.0] {
                model.zoom_camera.motion_blur = amount;
                let output = dir.join(format!("output-{fps}-{amount}.mp4"));
                let (sender, _receiver) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                match render(
                    &model,
                    &output,
                    &sender,
                    &cancel,
                    Encoder::X264,
                    ExportSettings {
                        fps: Some(fps),
                        ..ExportSettings::default()
                    },
                ) {
                    Ok(()) => (),
                    Err(End::Failed(message)) => panic!("export failed: {message}"),
                    Err(End::Cancelled) => panic!("export cancelled"),
                }
                videos.push(output);
            }
            let read_frame = |video: &PathBuf, time: &str| {
                let output = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
                    .args(["-hide_banner", "-loglevel", "error", "-ss", time, "-i"])
                    .arg(video)
                    .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout.len(), 320 * 180 * 3);
                output.stdout
            };
            let difference = |time: &str| {
                let first = read_frame(&videos[0], time);
                let second = read_frame(&videos[1], time);
                first
                    .iter()
                    .zip(second.iter())
                    .map(|(a, b)| a.abs_diff(*b) as u64)
                    .sum::<u64>() as f64
                    / first.len() as f64
            };
            let moving = difference("0.35");
            let resting = difference("1.0");
            assert!(moving > 1.0, "{fps} fps motion did not blur: {moving}");
            assert!(
                moving > resting * 2.0,
                "{fps} fps rest changed too much: moving={moving}, resting={resting}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selected_settings_reach_the_mp4_encoder() {
        if std::env::var("APEXSHOT_EXPORT_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "apexshot-export-options-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=320x180:r=30:d=0.5",
                "-threads",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("ffmpeg should be available");
        assert!(generated.success());
        let model = ExportModel {
            source,
            source_size: (320, 180),
            trim_start: 0.0,
            trim_end: 0.5,
            cuts: vec![0.25],
            zooms: Vec::new(),
            zoom_camera: zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 20,
        };
        for fps in [24, 60] {
            let settings = ExportSettings {
                fps: Some(fps),
                resolution: Some(90),
                ..ExportSettings::default()
            };
            let output = dir.join(format!("output-{fps}.mp4"));
            let (sender, _receiver) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            if let Err(end) = render(&model, &output, &sender, &cancel, Encoder::X264, settings) {
                match end {
                    End::Failed(message) => panic!("MP4 export failed: {message}"),
                    End::Cancelled => panic!("export cancelled itself"),
                }
            }
            let probe = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-count_frames",
                    "-show_entries",
                    "stream=width,height,nb_read_frames",
                    "-of",
                    "json",
                ])
                .arg(&output)
                .output()
                .unwrap();
            assert!(probe.status.success());
            let data: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let stream = &data["streams"][0];
            assert_eq!(stream["width"], 160);
            assert_eq!(stream["height"], 90);
            assert_eq!(
                stream["nb_read_frames"]
                    .as_str()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap(),
                fps / 2
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "manual recording comparison"]
    fn export_recording_for_manual_comparison() {
        let source = PathBuf::from(std::env::var_os("APEXSHOT_COMPARISON_CLIP").unwrap());
        let output = PathBuf::from(std::env::var_os("APEXSHOT_COMPARISON_OUTPUT").unwrap());
        assert!(
            !output.exists(),
            "refusing to overwrite the comparison output"
        );
        let width = std::env::var("APEXSHOT_COMPARISON_WIDTH")
            .unwrap()
            .parse()
            .unwrap();
        let height = std::env::var("APEXSHOT_COMPARISON_HEIGHT")
            .unwrap()
            .parse()
            .unwrap();
        let duration = ffmpeg::probe_duration(&source).unwrap();
        let sidecar_path = PathBuf::from(std::env::var_os("APEXSHOT_COMPARISON_SIDECAR").unwrap());
        let sidecar = crate::recording::editor::sidecar::PointerSidecar::load(&sidecar_path)
            .expect("the recording must have a sidecar");
        let theme = match std::env::var("APEXSHOT_COMPARISON_THEME").as_deref() {
            Ok("macos") => crate::recording::editor::model::CursorTheme::Macos,
            _ => CursorSettings::default().theme,
        };
        let rotation_mode = match std::env::var("APEXSHOT_COMPARISON_ROTATION").as_deref() {
            Ok("directional") => CursorRotationMode::Directional,
            _ => CursorRotationMode::Velocity,
        };
        let rotation_ratio = std::env::var("APEXSHOT_COMPARISON_ROTATION_RATIO")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.5);
        let base_rotation_deg = std::env::var("APEXSHOT_COMPARISON_BASE_DEG")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let always_use_default = std::env::var("APEXSHOT_COMPARISON_ALWAYS_DEFAULT").is_ok();
        let cuts: Vec<f64> = std::env::var("APEXSHOT_COMPARISON_CUTS")
            .ok()
            .map(|text| {
                text.split(',')
                    .filter_map(|value| value.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let trim_start = std::env::var("APEXSHOT_COMPARISON_TRIM_START")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let trim_end = std::env::var("APEXSHOT_COMPARISON_TRIM_END")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(duration);
        eprintln!(
            "export: theme {theme:?} rotation {rotation_mode:?} ratio {rotation_ratio} base {base_rotation_deg} default {always_use_default} trim {trim_start}..{trim_end} cuts {cuts:?}"
        );
        if std::env::var("APEXSHOT_COMPARISON_DUMP").is_ok() {
            for t in [0.4, 0.8, 1.2, 2.2, 2.6, 2.9, 3.4, 3.9, 4.4] {
                eprintln!(
                    "rotation {t}: {:.1} deg",
                    sidecar
                        .cursor_rotation_at(t, rotation_mode, rotation_ratio, base_rotation_deg)
                        .to_degrees()
                );
            }
        }
        if std::env::var("APEXSHOT_COMPARISON_PLAN").is_ok() {
            let motion = crate::recording::editor::sidecar::CursorMotion {
                smooth: CursorSettings::default().smooth,
                speed: 1.0,
                ..crate::recording::editor::sidecar::CursorMotion::default()
            };
            let free = {
                let mut free = sidecar.clone();
                free.set_cut_times(&[]);
                free
            };
            let with_cuts = {
                let mut with_cuts = sidecar.clone();
                with_cuts.set_cut_times(&cuts);
                with_cuts
            };
            let text = std::env::var("APEXSHOT_COMPARISON_PLAN_TIMES")
                .unwrap_or_else(|_| "1.40,1.45,1.49,1.50,1.55".to_string());
            for t in text
                .split(',')
                .filter_map(|value| value.trim().parse::<f64>().ok())
            {
                let free_x = free.presented_at(t, motion).map(|frame| frame.x);
                let cut_x = with_cuts.presented_at(t, motion).map(|frame| frame.x);
                eprintln!("plan {t}: free {free_x:?} cut {cut_x:?}");
            }
        }
        let model = ExportModel {
            source,
            source_size: (width, height),
            trim_start,
            trim_end,
            cuts,
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings {
                theme,
                rotation_mode,
                rotate_on_x_movement_ratio: rotation_ratio,
                base_rotation_deg,
                always_use_default,
                ..CursorSettings::default()
            },
            sidecar: Some(sidecar),
            webcam: None,
            canvas: None,
            crf: 20,
        };
        let handle = start_export(model, output.clone());
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(180)) {
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
            webcam: None,
            canvas: None,
            crf: 23,
        };
        let handle = start_export(model, output.clone());
        loop {
            match handle.receiver.recv_timeout(Duration::from_secs(180)) {
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
            webcam: None,
            canvas: None,
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
        for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
        for (index, pixel) in frame.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
                cursor_id: None,
            })
            .collect();
        sidecar.clicks = vec![ClickSample {
            t: 0.2,
            x: 190.0,
            y: 72.0,
            button: 1,
            cursor_id: None,
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
                &mut fast,
                size,
                view,
                &sidecar,
                std::path::Path::new(""),
                settings,
                0.21,
                size,
                &mut layer,
            );
            let mut reference = frame.clone();
            legacy_draw_cursor(
                &mut reference,
                size,
                view,
                &sidecar,
                std::path::Path::new(""),
                settings,
                0.21,
                size,
            );
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
    #[allow(clippy::too_many_arguments)]
    fn legacy_draw_cursor(
        pixels: &mut [u8],
        size: (u32, u32),
        view: (f64, f64, f64, f64),
        sidecar: &crate::recording::editor::sidecar::PointerSidecar,
        source_path: &std::path::Path,
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
            rotation_mode: settings.rotation_mode,
            rotation_ratio: settings.rotate_on_x_movement_ratio,
            base_rotation_deg: settings.base_rotation_deg,
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
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let settings = settings.clamped();
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
            let capture = sidecar.cursor_capture(source_path, &frame);
            crate::recording::editor::cursor_sprite::draw_recorded(
                &context,
                x,
                y,
                1.0,
                super::press_cursor_scale(sidecar, time),
                &frame,
                capture.as_ref(),
                settings,
                frame.alpha,
            );
        }
        surface.flush();
        let data = surface.data().unwrap();
        for (target, source) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(data.as_chunks::<4>().0)
        {
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
                crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffmpeg_executable())
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
                webcam: None,
                canvas: None,
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
            crate::utils::hidden_command(crate::recording::editor::ffmpeg::ffprobe_executable())
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
    use crate::recording::editor::model::CursorTheme;
    use crate::recording::editor::sidecar::WebcamTrack;
    use crate::video_editor::webcam::{CameraLayout, LayerSettings, LayoutCue, LayoutSettings};
    use std::fs;
    use std::time::Duration;

    /// Generate a solid clip with the bundled ffmpeg, or None when there is
    /// none to run.
    fn solid_clip(path: &Path, colour: &str, size: &str, seconds: f64) -> bool {
        let status = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
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
        let output = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
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

    /// A clip whose left half is red and whose right half is green, so a
    /// mirrored layer reads as swapped colours.
    fn split_clip(path: &Path, size: &str, seconds: f64) -> bool {
        let mut parts = size.split('x');
        let half = parts
            .next()
            .and_then(|width| width.parse::<u32>().ok())
            .map(|width| width / 2)
            .unwrap_or(160);
        let height = parts
            .next()
            .and_then(|height| height.parse::<u32>().ok())
            .unwrap_or(320);
        let status = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=red:s={half}x{height}:d={seconds}:r=30"),
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=lime:s={half}x{height}:d={seconds}:r=30"),
            ])
            .args(["-filter_complex", "hstack=inputs=2", "-pix_fmt", "yuv420p"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        matches!(status, Ok(status) if status.success())
    }

    /// A clip with a bright centre stripe between two edge stripes, so a
    /// uniform cover crop shows the centre while a squeezed copy shows all
    /// three.
    fn striped_clip(path: &Path, seconds: f64) -> bool {
        let status = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=yellow:s=80x240:d={seconds}:r=30"),
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=blue:s=160x240:d={seconds}:r=30"),
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=yellow:s=80x240:d={seconds}:r=30"),
            ])
            .args(["-filter_complex", "hstack=inputs=3", "-pix_fmt", "yuv420p"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        matches!(status, Ok(status) if status.success())
    }

    /// The width and height of the file's first video stream.
    fn video_dimensions(path: &Path) -> Option<(u32, u32)> {
        let output = crate::utils::hidden_command(ffmpeg::ffprobe_executable())
            .args(["-v", "error", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=width,height"])
            .args(["-of", "csv=p=0"])
            .arg(path)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut parts = text.trim().split(',');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }

    /// Export one frame of `source` with `layer` and `layouts` baked in, of
    /// `name`.mp4.
    fn exported_frame(
        dir: &Path,
        source: &Path,
        track_path: &Path,
        name: &str,
        layer: LayerSettings,
        layouts: LayoutSettings,
    ) -> Vec<u8> {
        exported_frame_with_zooms(dir, source, track_path, name, layer, layouts, Vec::new())
    }

    /// [`exported_frame`] with `zooms` on the composition's zoom track.
    fn exported_frame_with_zooms(
        dir: &Path,
        source: &Path,
        track_path: &Path,
        name: &str,
        layer: LayerSettings,
        layouts: LayoutSettings,
        zooms: Vec<ZoomClip>,
    ) -> Vec<u8> {
        let output = dir.join(format!("{name}.mp4"));
        let model = ExportModel {
            source: source.to_path_buf(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 1.5,
            cuts: Vec::new(),
            zooms,
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: PointerSidecar::load_next_to_video(source),
            webcam: Some(webcam::WebcamExport {
                path: track_path.to_path_buf(),
                settings: layer,
                layouts,
                track_seconds: 2.0,
                start: 0.0,
                video_seconds: 2.0,
                last_frame: 1.95,
                source_size: (320, 320),
            }),
            canvas: None,
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
        let _ = fs::remove_file(&output);
        frame
    }

    #[test]
    fn the_webcam_layer_composites_where_the_panel_puts_it() {
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
        let Some(track_path) = webcam_track_file(&source) else {
            eprintln!("skipping the webcam export test: no sidecar directory");
            return;
        };
        if let Some(parent) = track_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !split_clip(&track_path, "320x320", 2.0) {
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
            start: 0.0,
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

        // The bubble covers 0.6..0.95 of the frame, so half of it starts 8px in
        // and the other half ends 16px from the right edge.
        let bubble = LayerSettings {
            x: 0.6,
            y: 0.6,
            w: 0.35,
            h: 0.35,
            radius: 0.08,
            ..Default::default()
        };
        let frame = exported_frame(
            &dir,
            &source,
            &track_path,
            "bubble",
            bubble,
            LayoutSettings::default(),
        );
        let left = pixel(&frame, 320, 200, 180);
        let right = pixel(&frame, 320, 280, 180);
        assert!(
            left[0] > 150 && left[1] < 90,
            "the camera's left half should sit inside the bubble: {left:?}"
        );
        assert!(
            right[1] > 150 && right[0] < 90,
            "the camera's right half should sit inside the bubble: {right:?}"
        );
        let outside = pixel(&frame, 320, 20, 20);
        assert!(
            outside[0] < 70 && outside[2] < 70,
            "the footage should be untouched outside the bubble: {outside:?}"
        );

        // Mirroring swaps the two halves without moving the bubble.
        let mirrored = LayerSettings {
            mirror: true,
            ..bubble
        };
        let frame = exported_frame(
            &dir,
            &source,
            &track_path,
            "mirror",
            mirrored,
            LayoutSettings::default(),
        );
        let left = pixel(&frame, 320, 200, 180);
        let right = pixel(&frame, 320, 280, 180);
        assert!(
            left[1] > 150 && left[0] < 90,
            "a mirrored camera shows its right half on the left: {left:?}"
        );
        assert!(
            right[0] > 150 && right[1] < 90,
            "a mirrored camera shows its left half on the right: {right:?}"
        );

        // A fullscreen camera covers the frame instead of sitting in it, so the
        // corner the footage owns in every other layer is the camera here.
        let fullscreen = LayerSettings {
            fullscreen: true,
            ..bubble
        };
        let frame = exported_frame(
            &dir,
            &source,
            &track_path,
            "fullscreen",
            fullscreen,
            LayoutSettings::default(),
        );
        let corner = pixel(&frame, 320, 10, 10);
        assert!(
            corner[0] > 150,
            "a fullscreen camera should reach the corner of the frame: {corner:?}"
        );

        let _ = fs::remove_file(&sidecar_path);
        let _ = fs::remove_file(&track_path);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_camera_exports_at_the_studied_size_and_shrinks_in_a_zoom() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-webcam-size-export-{}-{:?}",
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
        let Some(track_path) = webcam_track_file(&source) else {
            eprintln!("skipping the webcam export test: no sidecar directory");
            return;
        };
        if let Some(parent) = track_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !split_clip(&track_path, "320x320", 2.0) {
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
            start: 0.0,
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

        // Studied sizing: a square camera at size 0.5 in a 320x240 frame is
        // 120 px, half the shortest side. Centred: x 100..220, y 60..180.
        let mut layer = LayerSettings {
            radius: 0.0,
            ..LayerSettings::default()
        };
        let frame_aspect = 320.0 / 240.0;
        layer.set_overlay_aspect(1.0, frame_aspect);
        layer.set_overlay_size(0.5, frame_aspect);
        layer.set_overlay_alignment(0.5, 0.5);
        let layouts = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            ..LayoutSettings::default()
        };
        let rest = exported_frame(&dir, &source, &track_path, "size", layer, layouts.clone());
        let red = pixel(&rest, 320, 108, 120);
        let green = pixel(&rest, 320, 212, 120);
        assert!(
            red[0] > 150 && red[1] < 90,
            "the camera's left edge is at 100px: {red:?}"
        );
        assert!(
            green[1] > 150 && green[0] < 90,
            "the camera's right edge is at 220px: {green:?}"
        );
        for (x, y) in [(94, 120), (226, 120), (160, 54), (160, 186)] {
            let outside = pixel(&rest, 320, x, y);
            assert!(
                outside[0] < 70 && outside[1] < 70,
                "outside the 120px square at ({x},{y}): {outside:?}"
            );
        }

        // Inside a zoom the camera is 0.8 of that, 96 px: x 112..208, y 72..168.
        let zoom = ZoomClip {
            start: 0.0,
            end: 1.5,
            scale: 2.0,
            center: (0.0, 0.0),
            hidden: false,
            mode: crate::video_editor::timeline::ZoomMode::Auto,
            instant: false,
            anchor: None,
        };
        let zoomed = exported_frame_with_zooms(
            &dir,
            &source,
            &track_path,
            "zoomed",
            layer,
            layouts,
            vec![zoom],
        );
        let red = pixel(&zoomed, 320, 118, 120);
        let green = pixel(&zoomed, 320, 202, 120);
        assert!(
            red[0] > 150 && red[1] < 90,
            "the zoomed camera's left edge is at 112px: {red:?}"
        );
        assert!(
            green[1] > 150 && green[0] < 90,
            "the zoomed camera's right edge is at 208px: {green:?}"
        );
        for (x, y) in [(106, 120), (214, 120), (160, 66), (160, 174)] {
            let outside = pixel(&zoomed, 320, x, y);
            assert!(
                outside[0] < 70 && outside[1] < 70,
                "outside the 96px square at ({x},{y}): {outside:?}"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_cues_run_on_the_composition_clock() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-webcam-cues-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let source = dir.join("screen.mp4");
        if !solid_clip(&source, "0x1e1e1e", "320x240", 5.0) {
            eprintln!("skipping the layout cue test: ffmpeg is unavailable");
            return;
        }
        let Some(track_path) = webcam_track_file(&source) else {
            eprintln!("skipping the layout cue test: no sidecar directory");
            return;
        };
        if let Some(parent) = track_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !split_clip(&track_path, "320x320", 4.0) {
            return;
        }
        let track = WebcamTrack {
            file: track_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            width: 320,
            height: 320,
            duration: 4.0,
            start: 0.0,
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

        let layouts = LayoutSettings {
            default_layout: CameraLayout::ScreenOnly,
            cues: vec![
                LayoutCue {
                    at: 0.2,
                    layout: CameraLayout::CameraOverlay,
                },
                LayoutCue {
                    at: 0.7,
                    layout: CameraLayout::ScreenOnly,
                },
                LayoutCue {
                    at: 1.2,
                    layout: CameraLayout::SplitScreen,
                },
                LayoutCue {
                    at: 1.7,
                    layout: CameraLayout::FullscreenCamera,
                },
                LayoutCue {
                    at: 2.2,
                    layout: CameraLayout::CameraCutout,
                },
            ],
            transition_seconds: 0.12,
            ..LayoutSettings::default()
        };
        let bubble = LayerSettings {
            x: 0.6,
            y: 0.6,
            w: 0.35,
            h: 0.35,
            radius: 0.08,
            ..Default::default()
        };
        let output = dir.join("cues.mp4");
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 2.0,
            trim_end: 4.5,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: PointerSidecar::load_next_to_video(&source),
            webcam: Some(webcam::WebcamExport {
                path: track_path.clone(),
                settings: bubble,
                layouts,
                track_seconds: 4.0,
                start: 0.0,
                video_seconds: 4.0,
                last_frame: 3.95,
                source_size: (320, 320),
            }),
            canvas: None,
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

        let base = frame_rgb(&output, 0.1, (320, 240)).expect("the export decodes");
        assert!(
            pixel(&base, 320, 10, 10)[0] < 70,
            "the base layout is screen only: {:?}",
            pixel(&base, 320, 10, 10)
        );

        let overlay = frame_rgb(&output, 0.4, (320, 240)).expect("the export decodes");
        let inside = pixel(&overlay, 320, 270, 200);
        assert!(
            inside[1] > 150 && inside[0] < 90,
            "the first cue restores the camera bubble: {inside:?}"
        );

        let screen = frame_rgb(&output, 0.9, (320, 240)).expect("the export decodes");
        let washed = pixel(&screen, 320, 270, 200);
        assert!(
            washed[0] < 70 && washed[1] < 70 && washed[2] < 70,
            "the screen-only cue removes the camera pixels: {washed:?}"
        );

        let split = frame_rgb(&output, 1.4, (320, 240)).expect("the export decodes");
        let screen_side = pixel(&split, 320, 80, 120);
        assert!(
            screen_side.iter().all(|channel| *channel < 45),
            "the split's screen side holds the footage: {screen_side:?}"
        );
        let left = pixel(&split, 320, 176, 120);
        let right = pixel(&split, 320, 272, 120);
        assert!(
            left[0] > 150 && left[1] < 90,
            "the camera's red half starts the split's camera: {left:?}"
        );
        assert!(
            right[1] > 150 && right[0] < 90,
            "the camera's green half ends the split's camera: {right:?}"
        );

        let fullscreen = frame_rgb(&output, 1.9, (320, 240)).expect("the export decodes");
        assert!(
            pixel(&fullscreen, 320, 40, 120)[0] > 150,
            "the last cue fills the frame with the camera: {:?}",
            pixel(&fullscreen, 320, 40, 120)
        );
        assert!(
            pixel(&fullscreen, 320, 280, 120)[1] > 150,
            "the camera keeps its own aspect across the frame"
        );

        let cutout = frame_rgb(&output, 2.4, (320, 240)).expect("the export decodes");
        let transparent = pixel(&cutout, 320, 160, 200);
        assert!(
            transparent.iter().all(|channel| *channel < 45),
            "the cutout's matte leaves the screen where no person is: {transparent:?}"
        );

        let _ = fs::remove_file(&sidecar_path);
        let _ = fs::remove_file(&track_path);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_split_screen_covers_its_pane_without_letterbox() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-webcam-cover-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let source = dir.join("screen.mp4");
        if !striped_clip(&source, 2.0) {
            eprintln!("skipping the split cover test: ffmpeg is unavailable");
            return;
        }
        let Some(track_path) = webcam_track_file(&source) else {
            eprintln!("skipping the split cover test: no sidecar directory");
            return;
        };
        if let Some(parent) = track_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !split_clip(&track_path, "320x320", 2.0) {
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
            start: 0.0,
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

        let output = dir.join("split.mp4");
        let model = ExportModel {
            source: source.clone(),
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 1.5,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: PointerSidecar::load_next_to_video(&source),
            webcam: Some(webcam::WebcamExport {
                path: track_path.clone(),
                settings: LayerSettings::default(),
                layouts: LayoutSettings {
                    default_layout: CameraLayout::SplitScreen,
                    ..LayoutSettings::default()
                },
                track_seconds: 2.0,
                start: 0.0,
                video_seconds: 2.0,
                last_frame: 1.95,
                source_size: (320, 320),
            }),
            canvas: None,
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

        assert_eq!(video_dimensions(&output), Some((320, 240)));
        let frame = frame_rgb(&output, 0.5, (320, 240)).expect("the export decodes");
        assert_eq!(frame.len(), 320 * 240 * 3);

        for y in [10, 120, 230] {
            for x in [20, 145] {
                let px = pixel(&frame, 320, x, y);
                assert!(
                    px[2] > 150 && px[0] < 90 && px[1] < 90,
                    "the screen pane shows its centre stripe at ({x},{y}): {px:?}"
                );
            }
        }

        for y in [10, 120, 230] {
            let camera = pixel(&frame, 320, 200, y);
            assert!(
                camera[0] > 150 && camera[1] < 90,
                "the camera fills the split's pane at y={y}: {camera:?}"
            );
        }
        let far = pixel(&frame, 320, 280, 120);
        assert!(
            far[1] > 150 && far[0] < 90,
            "the camera reaches the right edge of its pane: {far:?}"
        );

        let _ = fs::remove_file(&sidecar_path);
        let _ = fs::remove_file(&track_path);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_export_paints_a_captured_png_at_its_recording_hotspot() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorAsset, CursorKind, PointerSample, PointerSidecar,
        };
        use gtk4::cairo::{Format, ImageSurface};

        let video = std::env::temp_dir().join(format!(
            "apexshot-export-cursor-{}-{:?}.mp4",
            std::process::id(),
            std::thread::current().id()
        ));
        let sidecar_path = PointerSidecar::sidecar_path(&video);
        let folder_name = format!(
            "{}.cursors",
            sidecar_path
                .file_name()
                .expect("the sidecar has a file name")
                .to_string_lossy()
        );
        let folder = sidecar_path.parent().unwrap().join(&folder_name);
        std::fs::create_dir_all(&folder).unwrap();
        let mut image = image::RgbaImage::new(32, 32);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        image.save(folder.join("hand.png")).unwrap();

        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.cursor_images = Some(folder_name);
        sidecar.cursors.push(CursorAsset {
            id: "hand-id".to_string(),
            file: "hand.png".to_string(),
            width: 32,
            height: 32,
            hotspot_x: 1,
            hotspot_y: 1,
            kind: CursorKind::Hand,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 50.0,
            y: 40.0,
            kind: CursorKind::Hand,
            cursor_id: Some("hand-id".to_string()),
        });

        let size = (100u32, 100u32);
        let mut pixels = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[0, 0, 0, 255]);
        }
        let mut layer = ImageSurface::create(Format::ARgb32, size.0 as i32, size.1 as i32).unwrap();
        let settings = CursorSettings {
            smooth: 0.0,
            theme: CursorTheme::Macos,
            ..CursorSettings::default()
        };
        super::draw_cursor(
            &mut pixels,
            size,
            (0.0, 0.0, 100.0, 100.0),
            &sidecar,
            &video,
            settings,
            0.0,
            size,
            &mut layer,
        );

        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in 0..size.1 as usize {
            for x in 0..size.0 as usize {
                let p = &pixels[(y * size.0 as usize + x) * 4..][..4];
                if p[0] > 180 && p[1] < 60 && p[2] < 60 {
                    bounds = Some(match bounds {
                        Some((min_x, min_y, max_x, max_y)) => {
                            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                        }
                        None => (x, y, x, y),
                    });
                }
            }
        }
        assert_eq!(bounds, Some((49, 39, 95, 85)));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_export_finds_the_cursor_png_beside_a_clean_screen_file() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorAsset, CursorKind, PointerSample, PointerSidecar,
        };
        use gtk4::cairo::{Format, ImageSurface};

        let video = std::env::temp_dir().join(format!(
            "apexshot-export-clean-cursor-{}-{:?}.mp4",
            std::process::id(),
            std::thread::current().id()
        ));
        let clean = video.with_file_name("clean-screen.mp4");
        let sidecar_path = PointerSidecar::sidecar_path(&video);
        let folder_name = format!(
            "{}.cursors",
            sidecar_path
                .file_name()
                .expect("the sidecar has a file name")
                .to_string_lossy()
        );
        let folder = sidecar_path.parent().unwrap().join(&folder_name);
        std::fs::create_dir_all(&folder).unwrap();
        let mut image = image::RgbaImage::new(32, 32);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        image.save(folder.join("hand.png")).unwrap();

        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.cursor_images = Some(folder_name);
        sidecar.cursors.push(CursorAsset {
            id: "hand-id".to_string(),
            file: "hand.png".to_string(),
            width: 32,
            height: 32,
            hotspot_x: 1,
            hotspot_y: 1,
            kind: CursorKind::Hand,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 50.0,
            y: 40.0,
            kind: CursorKind::Hand,
            cursor_id: Some("hand-id".to_string()),
        });
        sidecar.write(&sidecar_path).unwrap();
        let loaded = PointerSidecar::load(&sidecar_path).unwrap();

        let size = (100u32, 100u32);
        let mut pixels = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[0, 0, 0, 255]);
        }
        let mut layer = ImageSurface::create(Format::ARgb32, size.0 as i32, size.1 as i32).unwrap();
        let settings = CursorSettings {
            smooth: 0.0,
            theme: CursorTheme::Macos,
            ..CursorSettings::default()
        };
        super::draw_cursor(
            &mut pixels,
            size,
            (0.0, 0.0, 100.0, 100.0),
            &loaded,
            &clean,
            settings,
            0.0,
            size,
            &mut layer,
        );

        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in 0..size.1 as usize {
            for x in 0..size.0 as usize {
                let p = &pixels[(y * size.0 as usize + x) * 4..][..4];
                if p[0] > 180 && p[1] < 60 && p[2] < 60 {
                    bounds = Some(match bounds {
                        Some((min_x, min_y, max_x, max_y)) => {
                            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                        }
                        None => (x, y, x, y),
                    });
                }
            }
        }
        assert_eq!(bounds, Some((49, 39, 95, 85)));
        let _ = std::fs::remove_file(&sidecar_path);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The track lands beside the pointer sidecar, exactly as the recorder
    /// writes it.
    fn webcam_track_file(source: &Path) -> Option<PathBuf> {
        let sidecar = PointerSidecar::sidecar_path(source);
        let stem = sidecar.file_name()?.to_string_lossy().into_owned();
        sidecar
            .parent()
            .map(|dir| dir.join(format!("{stem}-camera.mp4")))
    }

    #[test]
    #[ignore = "integration: shells out to ffmpeg"]
    fn a_plain_export_passes_the_decoded_frame_through() {
        let dir = std::env::temp_dir().join(format!("apexshot-passthrough-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        if !solid_clip(&source, "red", "320x240", 2.0) {
            eprintln!("skipping: ffmpeg could not make the clip");
            return;
        }
        let output = dir.join("output.mp4");
        let model = ExportModel {
            source,
            source_size: (320, 240),
            trim_start: 0.0,
            trim_end: 1.0,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: None,
            crf: 18,
        };
        let settings = ExportSettings {
            fps: Some(30),
            hardware: false,
            ..ExportSettings::default()
        };
        let (sender, _receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        render(&model, &output, &sender, &cancel, Encoder::X264, settings)
            .unwrap_or_else(|error| panic!("export failed: {error:?}"));
        let frame = frame_rgb(&output, 0.5, (320, 240)).expect("the export should decode");
        let [red, green, blue] = pixel(&frame, 320, 160, 120);
        assert!(
            red > 150 && green < 80 && blue < 80,
            "the frame lost its colour: {red}, {green}, {blue}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "integration: shells out to ffmpeg"]
    fn a_canvas_export_frames_the_video_on_its_background() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-canvas-export-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let generated = crate::utils::hidden_command(ffmpeg::ffmpeg_executable())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=320x180:r=30:d=1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("ffmpeg should be available");
        assert!(generated.success());
        let size = (400u32, 260u32);
        let mut background = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for pixel in background.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[0, 200, 0, 255]);
        }
        let output = dir.join("canvas.mp4");
        let model = ExportModel {
            source,
            source_size: (320, 180),
            trim_start: 0.0,
            trim_end: 1.0,
            cuts: Vec::new(),
            zooms: Vec::new(),
            zoom_camera: crate::video_editor::zoom::ZoomCameraSettings::default(),
            cursor: CursorSettings::default(),
            sidecar: None,
            webcam: None,
            canvas: Some(CanvasLayout {
                size,
                video: (320, 180),
                radius: 24.0,
                background: Arc::new(background),
            }),
            crf: 18,
        };
        let settings = ExportSettings {
            fps: Some(30),
            hardware: false,
            ..ExportSettings::default()
        };
        let (sender, _receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        render(&model, &output, &sender, &cancel, Encoder::X264, settings)
            .unwrap_or_else(|error| panic!("export failed: {error:?}"));
        let frame = frame_rgb(&output, 0.5, size).expect("the export should decode");
        assert_eq!(frame.len(), size.0 as usize * size.1 as usize * 3);
        let [red, green, _] = pixel(&frame, size.0, 4, 4);
        assert!(
            green > 150 && red < 80,
            "the padding lost its fill: {red}, {green}"
        );
        let [red, green, _] = pixel(&frame, size.0, 200, 130);
        assert!(
            red > 150 && green < 80,
            "the video centre is not the video: {red}, {green}"
        );
        let [red, green, _] = pixel(&frame, size.0, 41, 41);
        assert!(
            green > 150 && red < 80,
            "the corner was not rounded: {red}, {green}"
        );
        let [red, green, _] = pixel(&frame, size.0, 200, 42);
        assert!(
            red > 150 && green < 80,
            "the video's top edge is missing: {red}, {green}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
