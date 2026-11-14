//! The output canvas around the video: the background fill, the padding that
//! insets the video, a fixed frame aspect, and the video card's rounded
//! corners.
//!
//! The render paths produce the video rect in NV12 exactly as they would
//! without a canvas. [`FramedSink`] then lays it onto a background converted to
//! NV12 once per range: the video rows are copied into the centre and only the
//! anti-aliased corner pixels are blended. Passthrough and the GPU crop keep
//! working with a background, because neither ever sees the canvas.

use super::{crop_scale_into, frame_source_time, End, ExportModel, RenderPlan, Sink, SourceSpan};
use crate::video_editor::keystrokes::{self, Caption};
use crate::video_editor::pixels::{Nv12Converter, YuvSpace};
use crate::video_editor::zoom::StageRect;
use gtk4::cairo::{Context, Format, ImageSurface};
use rayon::prelude::*;
use std::sync::Arc;

/// The canvas the stage shows, measured at the editor's canvas size.
#[derive(Clone)]
pub(in crate::video_editor) struct CanvasLayout {
    /// The canvas in pixels: the export size before any resolution pick.
    pub size: (u32, u32),
    /// The video rect, centred in the canvas. Both edges are even.
    pub video: (u32, u32),
    /// The video card's corner radius, in canvas pixels.
    pub radius: f64,
    /// The zoom area, the stage inside the padding, in canvas pixels at
    /// `size`: x, y, width, height. A zoomed card stays covering it.
    pub area: (u32, u32, u32, u32),
    /// The fill as opaque RGBA at `size`, row-major with no padding.
    pub background: Arc<Vec<u8>>,
}

/// The zoomed card for one frame, in pixels of the export canvas.
#[derive(Clone, Copy, Debug)]
pub(super) struct CardFrame {
    /// Where the card is drawn.
    pub rect: StageRect,
    /// The background's scale and its pivot, as fractions of the canvas.
    pub background_zoom: f64,
    pub pivot: (f64, f64),
}

/// Whether `card` has left `rest`, with the preview's tolerance.
pub(super) fn card_moved(card: StageRect, rest: StageRect) -> bool {
    (card.0 - rest.0).abs() > 0.01
        || (card.1 - rest.1).abs() > 0.01
        || (card.2 - rest.2).abs() > 0.01
        || (card.3 - rest.3).abs() > 0.01
}

/// Coverage (0..=255) of the pixel at (x, y) by a rounded rectangle, from the
/// signed distance at the pixel centre with a one-pixel ramp.
fn rounded_coverage(x: usize, y: usize, rect: StageRect, radius: f64) -> u32 {
    let half_w = rect.2 / 2.0;
    let half_h = rect.3 / 2.0;
    let qx = (x as f64 + 0.5 - (rect.0 + half_w)).abs() - (half_w - radius);
    let qy = (y as f64 + 0.5 - (rect.1 + half_h)).abs() - (half_h - radius);
    let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
    ((0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u32
}

/// Blend `source` over `target` on one row between `from` and `to`, where
/// `alpha(x)` is the coverage at column x. Coverage is convex along a row, so
/// only the ends are evaluated until they reach full coverage; the middle is
/// copied.
fn blend_row(
    target: &mut [u8],
    source: &[u8],
    from: usize,
    to: usize,
    alpha: impl Fn(usize) -> u32,
) {
    let mut start = from;
    while start < to {
        let a = alpha(start);
        if a >= 255 {
            break;
        }
        if a > 0 {
            target[start] = blend(target[start], source[start], a);
        }
        start += 1;
    }
    let mut end = to;
    while end > start {
        let a = alpha(end - 1);
        if a >= 255 {
            break;
        }
        if a > 0 {
            target[end - 1] = blend(target[end - 1], source[end - 1], a);
        }
        end -= 1;
    }
    target[start..end].copy_from_slice(&source[start..end]);
}

/// The same for an interleaved UV row: `alpha(pair)` covers bytes
/// `2 * pair` and `2 * pair + 1`.
fn blend_chroma_row(
    target: &mut [u8],
    source: &[u8],
    from: usize,
    to: usize,
    alpha: impl Fn(usize) -> u32,
) {
    let mut start = from;
    while start < to {
        let a = alpha(start);
        if a >= 255 {
            break;
        }
        if a > 0 {
            for byte in [start * 2, start * 2 + 1] {
                target[byte] = blend(target[byte], source[byte], a);
            }
        }
        start += 1;
    }
    let mut end = to;
    while end > start {
        let a = alpha(end - 1);
        if a >= 255 {
            break;
        }
        if a > 0 {
            for byte in [(end - 1) * 2, (end - 1) * 2 + 1] {
                target[byte] = blend(target[byte], source[byte], a);
            }
        }
        end -= 1;
    }
    target[start * 2..end * 2].copy_from_slice(&source[start * 2..end * 2]);
}

/// The video rect at an export size. A resolution pick scales the canvas, so
/// the video rect scales with it and stays even for the encoder's 4:2:0.
pub(super) fn video_size(layout: &CanvasLayout, out: (u32, u32)) -> (u32, u32) {
    if out == layout.size {
        return (layout.video.0.min(out.0), layout.video.1.min(out.1));
    }
    let scale = f64::from(out.0) / f64::from(layout.size.0.max(1));
    let even = |value: u32, max: u32| {
        let scaled = (f64::from(value) * scale).round().max(2.0) as u32;
        (scaled & !1).min(max & !1).max(2)
    };
    (even(layout.video.0, out.0), even(layout.video.1, out.1))
}

/// The canvas fill behind the video rect, opaque RGBA at [`video_size`], so a
/// stage wider than its screen shows the background in the gaps.
pub(super) fn video_background(layout: &CanvasLayout, out: (u32, u32)) -> Vec<u8> {
    let video = video_size(layout, out);
    let scaled;
    let rgba: &[u8] = if out == layout.size {
        &layout.background
    } else {
        let mut buffer = vec![0u8; out.0 as usize * out.1 as usize * 4];
        let full = (0.0, 0.0, f64::from(layout.size.0), f64::from(layout.size.1));
        crop_scale_into(&layout.background, layout.size, full, out, &mut buffer);
        scaled = buffer;
        &scaled
    };
    let left = (((out.0 - video.0) / 2) & !1) as usize;
    let top = (((out.1 - video.1) / 2) & !1) as usize;
    let stride = out.0 as usize * 4;
    let row = video.0 as usize * 4;
    let mut background = vec![0u8; row * video.1 as usize];
    for y in 0..video.1 as usize {
        let start = (top + y) * stride + left * 4;
        background[y * row..(y + 1) * row].copy_from_slice(&rgba[start..start + row]);
    }
    background
}

/// Lays each video frame onto the canvas.
pub(super) struct Framer {
    out: (usize, usize),
    video: (usize, usize),
    /// Top-left of the video rect, even on both axes so chroma lines up.
    origin: (usize, usize),
    background: Vec<u8>,
    /// The RGBA background at the export size, for the zoomed card's own
    /// background pass.
    background_rgba: Vec<u8>,
    radius: usize,
    /// The video rect at rest and the zoom area, in export pixels.
    rest: StageRect,
    area: StageRect,
    /// The zoomed background, NV12, cached by its scale and pivot.
    zoomed: Option<((i64, i64, i64), Vec<u8>)>,
    scratch: Vec<u8>,
    /// Coverage of the top-left corner, `radius` rows of `radius` bytes; the
    /// other three corners mirror it.
    corner: Vec<u8>,
}

impl Framer {
    pub(super) fn new(
        layout: &CanvasLayout,
        out: (u32, u32),
        converter: &Nv12Converter,
    ) -> Result<Self, End> {
        let expected = layout.size.0 as usize * layout.size.1 as usize * 4;
        if layout.background.len() != expected || out.0 < 2 || out.1 < 2 {
            return Err(End::Failed(
                "the export background does not match its canvas".to_string(),
            ));
        }
        let video = video_size(layout, out);
        let scaled;
        let rgba: &[u8] = if out == layout.size {
            &layout.background
        } else {
            let mut buffer = vec![0u8; out.0 as usize * out.1 as usize * 4];
            let full = (0.0, 0.0, f64::from(layout.size.0), f64::from(layout.size.1));
            crop_scale_into(&layout.background, layout.size, full, out, &mut buffer);
            scaled = buffer;
            &scaled
        };
        let mut background = vec![0u8; out.0 as usize * out.1 as usize * 3 / 2];
        converter.convert_into(rgba, out, &mut background);
        let background_rgba = rgba.to_vec();
        let scale = f64::from(out.0) / f64::from(layout.size.0.max(1));
        let radius = if layout.radius.is_finite() && layout.radius > 0.0 {
            ((layout.radius * scale).round().max(0.0) as usize)
                .min(video.0 as usize / 2)
                .min(video.1 as usize / 2)
        } else {
            0
        };
        let origin = (
            ((out.0 - video.0) / 2) as usize & !1,
            ((out.1 - video.1) / 2) as usize & !1,
        );
        let scale_y = f64::from(out.1) / f64::from(layout.size.1.max(1));
        let area = (
            f64::from(layout.area.0) * scale,
            f64::from(layout.area.1) * scale_y,
            f64::from(layout.area.2) * scale,
            f64::from(layout.area.3) * scale_y,
        );
        Ok(Self {
            out: (out.0 as usize, out.1 as usize),
            video: (video.0 as usize, video.1 as usize),
            origin,
            background,
            background_rgba,
            radius,
            rest: (
                origin.0 as f64,
                origin.1 as f64,
                f64::from(video.0),
                f64::from(video.1),
            ),
            area,
            zoomed: None,
            scratch: Vec::new(),
            corner: corner_coverage(radius),
        })
    }

    pub(super) fn rest(&self) -> StageRect {
        self.rest
    }

    pub(super) fn area(&self) -> StageRect {
        self.area
    }

    fn video_bytes(&self) -> usize {
        self.video.0 * self.video.1 * 3 / 2
    }

    fn coverage(&self, x: usize, y: usize) -> u32 {
        let radius = self.radius;
        if radius == 0 {
            return 255;
        }
        let (width, height) = self.video;
        let column = if x < radius {
            x
        } else if x >= width - radius {
            width - 1 - x
        } else {
            return 255;
        };
        let row = if y < radius {
            y
        } else if y >= height - radius {
            height - 1 - y
        } else {
            return 255;
        };
        u32::from(self.corner[row * radius + column])
    }

    /// Write one canvas frame: the background, with `video` (NV12 at
    /// [`video_size`](Self::video_size)) laid into the centre.
    pub(super) fn compose(&self, video: &[u8], out: &mut [u8]) {
        let (out_w, out_h) = self.out;
        let (width, height) = self.video;
        let (left, top) = self.origin;
        let radius = self.radius;
        out.copy_from_slice(&self.background);
        for y in 0..height {
            let source = &video[y * width..(y + 1) * width];
            let start = (top + y) * out_w + left;
            let target = &mut out[start..start + width];
            if y >= radius && y < height - radius {
                target.copy_from_slice(source);
                continue;
            }
            for x in 0..width {
                target[x] = blend(target[x], source[x], self.coverage(x, y));
            }
        }
        let video_chroma = width * height;
        let out_chroma = out_w * out_h;
        for row in 0..height / 2 {
            let source = &video[video_chroma + row * width..video_chroma + (row + 1) * width];
            let start = out_chroma + (top / 2 + row) * out_w + left;
            let target = &mut out[start..start + width];
            let y = row * 2;
            if y >= radius && y + 1 < height - radius {
                target.copy_from_slice(source);
                continue;
            }
            for pair in 0..width / 2 {
                let x = pair * 2;
                let alpha = (self.coverage(x, y)
                    + self.coverage(x + 1, y)
                    + self.coverage(x, y + 1)
                    + self.coverage(x + 1, y + 1)
                    + 2)
                    / 4;
                target[x] = blend(target[x], source[x], alpha);
                target[x + 1] = blend(target[x + 1], source[x + 1], alpha);
            }
        }
    }

    /// Write one canvas frame for a zoomed card: the zoomed background, the
    /// card's footage over the whole canvas (`outer`, NV12 at the export size)
    /// inside the card's rounded shape, then `video` (NV12 at
    /// [`video_size`](Self::video_size)) on top through the same shape.
    pub(super) fn compose_card(
        &mut self,
        video: &[u8],
        card: CardFrame,
        outer: &[u8],
        converter: &Nv12Converter,
        out: &mut [u8],
    ) {
        let (out_w, out_h) = self.out;
        let base = out_w * out_h;

        if card.background_zoom > 1.0 + 1e-6 {
            let key = (
                (card.background_zoom * 1e4).round() as i64,
                (card.pivot.0 * 1e4).round() as i64,
                (card.pivot.1 * 1e4).round() as i64,
            );
            if !matches!(&self.zoomed, Some((cached, _)) if *cached == key) {
                let w = out_w as f64;
                let h = out_h as f64;
                let px = card.pivot.0.clamp(0.0, 1.0) * w;
                let py = card.pivot.1.clamp(0.0, 1.0) * h;
                let z = card.background_zoom;
                let view = (px - px / z, py - py / z, w / z, h / z);
                self.scratch.resize(out_w * out_h * 4, 0);
                crop_scale_into(
                    &self.background_rgba,
                    (out_w as u32, out_h as u32),
                    view,
                    (out_w as u32, out_h as u32),
                    &mut self.scratch,
                );
                let mut nv12 = vec![0u8; base * 3 / 2];
                converter.convert_into(&self.scratch, (out_w as u32, out_h as u32), &mut nv12);
                self.zoomed = Some((key, nv12));
            }
            let zoomed = self
                .zoomed
                .as_ref()
                .map(|(_, zoomed)| zoomed)
                .expect("the zoomed background was just built");
            out.copy_from_slice(zoomed);
        } else {
            out.copy_from_slice(&self.background);
        }

        let radius = (self.radius as f64 * card.rect.2 / self.rest.2.max(1.0))
            .min(card.rect.2 / 2.0)
            .min(card.rect.3 / 2.0)
            .max(0.0);

        let (width, height) = self.video;
        let (left, top) = self.origin;

        let x0 = card.rect.0.floor().max(0.0) as usize;
        let x1 = ((card.rect.0 + card.rect.2).ceil().max(0.0) as usize).min(out_w);
        let y0 = card.rect.1.floor().max(0.0) as usize;
        let y1 = ((card.rect.1 + card.rect.3).ceil().max(0.0) as usize).min(out_h);

        let rect = card.rect;
        let (luma, chroma) = out.split_at_mut(base);
        let (outer_luma, outer_chroma) = outer.split_at(base);
        luma.par_chunks_mut(out_w)
            .zip(outer_luma.par_chunks(out_w))
            .enumerate()
            .skip(y0)
            .take(y1.saturating_sub(y0))
            .for_each(|(y, (target, source))| {
                blend_row(target, source, x0, x1, |x| {
                    rounded_coverage(x, y, rect, radius)
                });
            });

        let (p0, p1) = (x0 / 2, x1.div_ceil(2).min(out_w / 2));
        let (r0, r1) = (y0 / 2, y1.div_ceil(2).min(out_h / 2));
        chroma
            .par_chunks_mut(out_w)
            .zip(outer_chroma.par_chunks(out_w))
            .enumerate()
            .skip(r0)
            .take(r1.saturating_sub(r0))
            .for_each(|(row, (target, source))| {
                let y = row * 2;
                let lower = if y + 1 < out_h { y + 1 } else { y };
                blend_chroma_row(target, source, p0, p1, |pair| {
                    let x = pair * 2;
                    (rounded_coverage(x, y, rect, radius)
                        + rounded_coverage(x + 1, y, rect, radius)
                        + rounded_coverage(x, lower, rect, radius)
                        + rounded_coverage(x + 1, lower, rect, radius)
                        + 2)
                        / 4
                });
            });

        let (video_luma, video_chroma) = video.split_at(width * height);
        luma.par_chunks_mut(out_w)
            .enumerate()
            .skip(top)
            .take(height)
            .for_each(|(cy, target)| {
                let source = &video_luma[(cy - top) * width..(cy - top + 1) * width];
                blend_row(&mut target[left..left + width], source, 0, width, |x| {
                    rounded_coverage(left + x, cy, rect, radius)
                });
            });
        chroma
            .par_chunks_mut(out_w)
            .enumerate()
            .skip(top / 2)
            .take(height / 2)
            .for_each(|(row, target)| {
                let r = row - top / 2;
                let source = &video_chroma[r * width..(r + 1) * width];
                let y = top + r * 2;
                blend_chroma_row(
                    &mut target[left..left + width],
                    source,
                    0,
                    width / 2,
                    |pair| {
                        let x = left + pair * 2;
                        (rounded_coverage(x, y, rect, radius)
                            + rounded_coverage(x + 1, y, rect, radius)
                            + rounded_coverage(x, y + 1, rect, radius)
                            + rounded_coverage(x + 1, y + 1, rect, radius)
                            + 2)
                            / 4
                    },
                );
            });
    }

    /// Write one canvas frame for a moving card: the zoomed background with
    /// the card's whole trail (`layer`, premultiplied RGBA at the export size)
    /// over it, so its leading and trailing edges smear across the background
    /// as they do in the preview.
    pub(super) fn compose_card_layer(
        &mut self,
        card: CardFrame,
        layer: &[u8],
        converter: &Nv12Converter,
        out: &mut [u8],
    ) {
        let (out_w, out_h) = self.out;
        let base = out_w * out_h;
        if layer.len() != base * 4 || out.len() != base * 3 / 2 {
            return;
        }
        // The same background the sharp card's own pass draws, kept in RGBA so
        // the layer's premultiplied edges blend against the fill.
        self.scratch.resize(base * 4, 0);
        if card.background_zoom > 1.0 + 1e-6 {
            let w = out_w as f64;
            let h = out_h as f64;
            let px = card.pivot.0.clamp(0.0, 1.0) * w;
            let py = card.pivot.1.clamp(0.0, 1.0) * h;
            let z = card.background_zoom;
            let view = (px - px / z, py - py / z, w / z, h / z);
            crop_scale_into(
                &self.background_rgba,
                (out_w as u32, out_h as u32),
                view,
                (out_w as u32, out_h as u32),
                &mut self.scratch,
            );
        } else {
            self.scratch.copy_from_slice(&self.background_rgba);
        }
        self.scratch
            .par_chunks_mut(out_w * 4)
            .zip(layer.par_chunks(out_w * 4))
            .for_each(|(row, over)| {
                for (pixel, source) in row
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(over.as_chunks::<4>().0)
                {
                    let alpha = u32::from(source[3]);
                    if alpha == 0 {
                        continue;
                    }
                    let keep = 255 - alpha;
                    for channel in 0..3 {
                        pixel[channel] = (u32::from(source[channel])
                            + u32::from(pixel[channel]) * keep / 255)
                            as u8;
                    }
                    pixel[3] = 255;
                }
            });
        converter.convert_into(&self.scratch, (out_w as u32, out_h as u32), out);
    }
}

/// Coverage of a quarter circle of `radius` centred on the corner's inner
/// point, one byte per pixel, sampled at pixel centres with a one-pixel ramp.
fn corner_coverage(radius: usize) -> Vec<u8> {
    let mut corner = vec![255u8; radius * radius];
    let r = radius as f64;
    for row in 0..radius {
        for column in 0..radius {
            let dx = r - (column as f64 + 0.5);
            let dy = r - (row as f64 + 0.5);
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = (r - distance + 0.5).clamp(0.0, 1.0);
            corner[row * radius + column] = (coverage * 255.0).round() as u8;
        }
    }
    corner
}

fn blend(background: u8, video: u8, alpha: u32) -> u8 {
    if alpha >= 255 {
        return video;
    }
    ((u32::from(background) * (255 - alpha) + u32::from(video) * alpha + 127) / 255) as u8
}

/// The encoder sink, with the canvas applied when the export has one. The
/// render paths ask it for a buffer and submit it exactly as they ask the
/// plain sink; with a canvas the buffer is the video rect, and submitting
/// lays it onto the canvas before it reaches the encoder.
pub(super) struct FramedSink {
    sink: Sink,
    framer: Option<Framer>,
    captions: Option<CaptionOverlay>,
    pending: u64,
    spare: Vec<Vec<u8>>,
}

impl FramedSink {
    pub(super) fn new(sink: Sink, framer: Option<Framer>) -> Self {
        Self {
            sink,
            framer,
            captions: None,
            pending: 0,
            spare: Vec::new(),
        }
    }

    pub(super) fn set_captions(&mut self, captions: Option<CaptionOverlay>) {
        self.captions = captions;
    }

    /// The canvas background at the export size, opaque RGBA, when the export
    /// has a canvas.
    pub(super) fn background_rgba(&self) -> Option<&[u8]> {
        self.framer
            .as_ref()
            .map(|framer| framer.background_rgba.as_slice())
    }

    /// The resting card's corner radius as a fraction of its width.
    pub(super) fn radius_fraction(&self) -> f32 {
        self.framer.as_ref().map_or(0.0, |framer| {
            (framer.radius as f64 / framer.rest.2.max(1.0)) as f32
        })
    }

    /// A buffer for a whole output frame the caller composites itself.
    pub(super) fn canvas_buffer(&mut self, index: u64) -> Result<Vec<u8>, End> {
        self.pending = index;
        self.sink.buffer(index)
    }

    /// Submit a whole output frame, bypassing the framer.
    pub(super) fn submit_canvas(&mut self, frame: Vec<u8>) -> Result<(), End> {
        self.submit_output(frame)
    }

    pub(super) fn buffer(&mut self, index: u64) -> Result<Vec<u8>, End> {
        self.pending = index;
        let Some(framer) = self.framer.as_ref() else {
            return self.sink.buffer(index);
        };
        let bytes = framer.video_bytes();
        Ok(self
            .spare
            .pop()
            .filter(|buffer| buffer.len() == bytes)
            .unwrap_or_else(|| vec![0u8; bytes]))
    }

    pub(super) fn submit(&mut self, frame: Vec<u8>) -> Result<(), End> {
        self.submit_with(frame, |_| Ok(()))
    }

    /// [`Self::submit`], running `overlay` on the whole output frame after
    /// framing and before the captions, for layers placed on the canvas.
    pub(super) fn submit_with(
        &mut self,
        frame: Vec<u8>,
        overlay: impl FnOnce(&mut [u8]) -> Result<(), End>,
    ) -> Result<(), End> {
        let Some(framer) = self.framer.as_ref() else {
            let mut frame = frame;
            overlay(&mut frame)?;
            return self.submit_output(frame);
        };
        let mut canvas = self.sink.buffer(self.pending)?;
        framer.compose(&frame, &mut canvas);
        self.spare.push(frame);
        overlay(&mut canvas)?;
        self.submit_output(canvas)
    }

    /// Every output frame goes through here once, so the captions reach every
    /// render path after framing.
    fn submit_output(&mut self, mut frame: Vec<u8>) -> Result<(), End> {
        if let Some(captions) = self.captions.as_mut() {
            captions.apply(self.pending, &mut frame)?;
        }
        self.sink.submit(frame)
    }

    /// The rest rect and the zoom area of the canvas, when the export has one.
    pub(super) fn card_geometry(&self) -> Option<(StageRect, StageRect)> {
        self.framer
            .as_ref()
            .map(|framer| (framer.rest(), framer.area()))
    }

    /// Submit a frame drawn on a zoomed card, then run `overlay` on the whole
    /// output frame as [`Self::submit_with`] does. `outer` is the card's
    /// footage over the whole canvas, NV12 at the export size.
    pub(super) fn submit_card_with(
        &mut self,
        frame: Vec<u8>,
        card: CardFrame,
        outer: &[u8],
        converter: &Nv12Converter,
        overlay: impl FnOnce(&mut [u8]) -> Result<(), End>,
    ) -> Result<(), End> {
        let Some(framer) = self.framer.as_mut() else {
            let mut frame = frame;
            overlay(&mut frame)?;
            return self.submit_output(frame);
        };
        let mut canvas = self.sink.buffer(self.pending)?;
        framer.compose_card(&frame, card, outer, converter, &mut canvas);
        self.spare.push(frame);
        overlay(&mut canvas)?;
        self.submit_output(canvas)
    }

    /// Submit a frame whose moving card was blurred whole over the canvas:
    /// `layer` is the card's trail, premultiplied RGBA at the export size.
    pub(super) fn submit_card_layer_with(
        &mut self,
        frame: Vec<u8>,
        card: CardFrame,
        layer: &[u8],
        converter: &Nv12Converter,
        overlay: impl FnOnce(&mut [u8]) -> Result<(), End>,
    ) -> Result<(), End> {
        let Some(framer) = self.framer.as_mut() else {
            let mut frame = frame;
            overlay(&mut frame)?;
            return self.submit_output(frame);
        };
        let mut canvas = self.sink.buffer(self.pending)?;
        framer.compose_card_layer(card, layer, converter, &mut canvas);
        self.spare.push(frame);
        overlay(&mut canvas)?;
        self.submit_output(canvas)
    }

    pub(super) fn finish(self) -> Result<(), End> {
        self.sink.finish()
    }
}

/// Where the caption strip starts, as a share of the frame height. The pill
/// is 7.5% of the height plus its padding and sits 3.3% above the bottom, so
/// it reaches about 13.5% up; the strip leaves room above that.
const CAPTION_TOP_SHARE: f64 = 0.82;
const CAPTION_MARGIN_ROWS: usize = 2;

/// Keyboard captions drawn onto the bottom strip of each output frame whose
/// source time shows one. The strip is converted to RGBA and back, so the
/// rows above it are never touched.
pub(super) struct CaptionOverlay {
    captions: Vec<Caption>,
    spans: Vec<SourceSpan>,
    fps: f64,
    size: (u32, u32),
    converter: Nv12Converter,
    strip: Vec<u8>,
    rgba: Vec<u8>,
    layer: Option<ImageSurface>,
}

impl CaptionOverlay {
    pub(super) fn new(model: &ExportModel, plan: &RenderPlan, space: YuvSpace) -> Option<Self> {
        let settings = model.effects.captions;
        if !settings.shortcuts {
            return None;
        }
        let sidecar = model.sidecar.as_ref()?;
        let captions: Vec<Caption> =
            keystrokes::captions(&sidecar.keys, settings.show_single_letters)
                .into_iter()
                .filter(|caption| {
                    plan.spans.iter().any(|span| {
                        caption.start < span.source_end && span.source_start < caption.end
                    })
                })
                .collect();
        (!captions.is_empty())
            .then(|| Self::from_captions(captions, &plan.spans, plan.fps, plan.output, space))
    }

    pub(super) fn from_captions(
        captions: Vec<Caption>,
        spans: &[SourceSpan],
        fps: f64,
        size: (u32, u32),
        space: YuvSpace,
    ) -> Self {
        Self {
            captions,
            spans: spans.to_vec(),
            fps,
            size,
            converter: Nv12Converter::new(space),
            strip: Vec::new(),
            rgba: Vec::new(),
            layer: None,
        }
    }

    pub(super) fn apply(&mut self, index: u64, frame: &mut [u8]) -> Result<(), End> {
        if held_frame(&self.spans, index) {
            return Ok(());
        }
        let Some(time) = frame_source_time(&self.spans, index, self.fps) else {
            return Ok(());
        };
        if !self
            .captions
            .iter()
            .any(|caption| caption.start <= time && time < caption.end)
        {
            return Ok(());
        }
        let width = self.size.0 as usize;
        let height = self.size.1 as usize;
        let top = ((height as f64 * CAPTION_TOP_SHARE).floor() as usize) & !1;
        let strip_start = top.saturating_sub(CAPTION_MARGIN_ROWS);
        let strip_rows = height - strip_start;
        let luma_len = width * strip_rows;
        let uv_start = width * height;

        self.strip.resize(luma_len * 3 / 2, 0);
        let (strip_luma, strip_chroma) = self.strip.split_at_mut(luma_len);
        strip_luma.copy_from_slice(&frame[strip_start * width..uv_start]);
        strip_chroma.copy_from_slice(
            &frame[uv_start + (strip_start / 2) * width..uv_start + (height / 2) * width],
        );
        self.rgba.resize(luma_len * 4, 0);
        self.converter.to_rgba_into(
            &self.strip,
            (self.size.0, strip_rows as u32),
            &mut self.rgba,
        );

        let mut layer =
            match self.layer.take().filter(|layer| {
                layer.width() == width as i32 && layer.height() == strip_rows as i32
            }) {
                Some(layer) => layer,
                None => ImageSurface::create(Format::ARgb32, width as i32, strip_rows as i32)
                    .map_err(caption_error)?,
            };
        let stride = layer.stride() as usize;
        {
            let mut data = layer.data().map_err(caption_error)?;
            for (row, source) in self.rgba.chunks_exact(width * 4).enumerate() {
                let line = &mut data[row * stride..row * stride + width * 4];
                for (pixel, rgba) in line
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(source.as_chunks::<4>().0)
                {
                    pixel.copy_from_slice(&[rgba[2], rgba[1], rgba[0], 255]);
                }
            }
        }
        layer.mark_dirty();
        {
            let context = Context::new(&layer).map_err(caption_error)?;
            context.translate(0.0, -(strip_start as f64));
            keystrokes::draw(&context, width as f64, height as f64, &self.captions, time);
        }
        layer.flush();
        {
            let data = layer.data().map_err(caption_error)?;
            for (row, target) in self.rgba.chunks_exact_mut(width * 4).enumerate() {
                let line = &data[row * stride..row * stride + width * 4];
                for (rgba, pixel) in target
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(line.as_chunks::<4>().0)
                {
                    rgba.copy_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
            }
        }
        self.layer = Some(layer);

        self.converter.convert_into(
            &self.rgba,
            (self.size.0, strip_rows as u32),
            &mut self.strip,
        );
        let rest = top - strip_start;
        frame[top * width..uv_start].copy_from_slice(&self.strip[rest * width..luma_len]);
        frame[uv_start + (top / 2) * width..uv_start + (height / 2) * width]
            .copy_from_slice(&self.strip[luma_len + (rest / 2) * width..]);
        Ok(())
    }
}

fn caption_error(error: impl std::fmt::Display) -> End {
    End::Failed(format!("could not draw the captions: {error}"))
}

/// Whether the global composition frame sits in a span that holds its first
/// source frame. A hold replays footage the captions have already played, so
/// painting them again would leave the last caption on screen for the whole
/// freeze.
fn held_frame(spans: &[SourceSpan], index: u64) -> bool {
    let mut remaining = index;
    for span in spans {
        if remaining < span.frames {
            return span.speed <= 0.0 || !span.speed.is_finite();
        }
        remaining -= span.frames;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        corner_coverage, rounded_coverage, video_size, CanvasLayout, CaptionOverlay, CardFrame,
        Framer,
    };
    use crate::video_editor::pixels::{Nv12Converter, YuvSpace};
    use std::sync::Arc;

    fn layout(size: (u32, u32), video: (u32, u32), radius: f64) -> CanvasLayout {
        CanvasLayout {
            size,
            video,
            radius,
            area: (0, 0, size.0, size.1),
            background: Arc::new(vec![0u8; size.0 as usize * size.1 as usize * 4]),
        }
    }

    #[test]
    fn the_video_rect_scales_with_a_resolution_pick() {
        let canvas = layout((1920, 1080), (1600, 900), 0.0);
        assert_eq!(video_size(&canvas, (1920, 1080)), (1600, 900));
        assert_eq!(video_size(&canvas, (1280, 720)), (1066, 600));
    }

    #[test]
    fn a_corner_is_clear_outside_and_solid_inside() {
        let corner = corner_coverage(8);
        assert_eq!(corner[0], 0);
        assert_eq!(corner[7 * 8 + 7], 255);
        assert!(corner.iter().any(|value| *value > 0 && *value < 255));
    }

    #[test]
    fn captions_redraw_only_the_bottom_strip_while_active() {
        let Some(frames) = crate::test_support::with_gtk(|| {
            let space = crate::video_editor::pixels::YuvSpace::from_probe(None, None);
            let spans = [super::SourceSpan {
                composition_start: 0.0,
                source_start: 0.0,
                source_end: 3.0,
                speed: 1.0,
                frames: 90,
            }];
            let captions = vec![super::Caption {
                start: 0.0,
                end: 1.0,
                keys: vec!["Ctrl".to_owned(), "S".to_owned()],
                count: 1,
            }];
            let mut overlay =
                CaptionOverlay::from_captions(captions, &spans, 30.0, (320, 240), space);
            let luma = 320 * 240;
            let plain = vec![128u8; luma * 3 / 2];
            let canvas: Vec<u8> = (0..luma * 3 / 2)
                .map(|index| {
                    if index < luma {
                        (40 + index % 200) as u8
                    } else {
                        128
                    }
                })
                .collect();
            [plain, canvas]
                .into_iter()
                .map(|source| {
                    let mut active = source.clone();
                    overlay.apply(0, &mut active).unwrap();
                    let mut inactive = source.clone();
                    overlay.apply(45, &mut inactive).unwrap();
                    (source, active, inactive)
                })
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        let luma = 320 * 240;
        for (source, active, inactive) in frames {
            assert_eq!(inactive, source);
            assert_eq!(active[..194 * 320], source[..194 * 320]);
            assert_eq!(active[luma..luma + 97 * 320], source[luma..luma + 97 * 320]);
            assert_ne!(active[212 * 320..luma], source[212 * 320..luma]);
        }
    }

    #[test]
    fn a_held_frame_does_not_paint_the_captions_again() {
        let Some(frames) = crate::test_support::with_gtk(|| {
            let space = crate::video_editor::pixels::YuvSpace::from_probe(None, None);
            let spans = [
                super::SourceSpan {
                    composition_start: 0.0,
                    source_start: 0.0,
                    source_end: 1.0,
                    speed: 1.0,
                    frames: 30,
                },
                super::SourceSpan {
                    composition_start: 1.0,
                    source_start: 0.966_667,
                    source_end: 0.966_667,
                    speed: 0.0,
                    frames: 30,
                },
            ];
            let captions = vec![super::Caption {
                start: 0.0,
                end: 1.0,
                keys: vec!["Ctrl".to_owned(), "S".to_owned()],
                count: 1,
            }];
            let mut overlay =
                CaptionOverlay::from_captions(captions, &spans, 30.0, (320, 240), space);
            let luma = 320 * 240;
            let canvas: Vec<u8> = (0..luma * 3 / 2)
                .map(|index| {
                    if index < luma {
                        (40 + index % 200) as u8
                    } else {
                        128
                    }
                })
                .collect();
            let mut kept = canvas.clone();
            overlay.apply(10, &mut kept).unwrap();
            let mut held = canvas.clone();
            overlay.apply(45, &mut held).unwrap();
            (canvas, kept, held)
        }) else {
            return;
        };
        let (canvas, kept, held) = frames;
        assert_eq!(held, canvas, "the held frame repainted the captions");
        assert_ne!(kept, canvas, "the kept frame should paint its caption");
    }

    #[test]
    fn the_video_lands_centred_on_the_background() {
        let converter = Nv12Converter::new(YuvSpace::from_probe(None, None));
        let canvas = layout((64, 48), (32, 24), 0.0);
        let framer = Framer::new(&canvas, (64, 48), &converter).unwrap();
        let (width, height) = framer.video;
        assert_eq!((width, height), (32, 24));
        let video = vec![200u8; width * height * 3 / 2];
        let mut out = vec![0u8; 64 * 48 * 3 / 2];
        framer.compose(&video, &mut out);
        let background_luma = framer.background[0];
        assert_ne!(background_luma, 200);
        assert_eq!(out[0], background_luma);
        // The video rect starts at (16, 12).
        assert_eq!(out[12 * 64 + 16], 200);
        assert_eq!(out[12 * 64 + 15], background_luma);
        assert_eq!(out[35 * 64 + 47], 200);
        assert_eq!(out[36 * 64 + 47], background_luma);
    }

    #[test]
    fn rounded_corners_show_the_background() {
        let converter = Nv12Converter::new(YuvSpace::from_probe(None, None));
        let canvas = layout((64, 48), (32, 24), 6.0);
        let framer = Framer::new(&canvas, (64, 48), &converter).unwrap();
        let video = vec![200u8; 32 * 24 * 3 / 2];
        let mut out = vec![0u8; 64 * 48 * 3 / 2];
        framer.compose(&video, &mut out);
        let background_luma = framer.background[0];
        // The video rect's own corner pixel is outside the rounding.
        assert_eq!(out[12 * 64 + 16], background_luma);
        // Its edge midpoints and centre are video.
        assert_eq!(out[12 * 64 + 32], 200);
        assert_eq!(out[24 * 64 + 16], 200);
        assert_eq!(out[24 * 64 + 32], 200);
    }

    #[test]
    fn a_grown_card_covers_the_padding_with_footage() {
        let converter = Nv12Converter::new(YuvSpace::from_probe(None, None));
        let canvas = layout((64, 48), (32, 24), 0.0);
        let mut framer = Framer::new(&canvas, (64, 48), &converter).unwrap();
        let video = vec![200u8; 32 * 24 * 3 / 2];
        let mut outer = vec![0u8; 64 * 48 * 3 / 2];
        outer[..64 * 48].fill(120);
        outer[64 * 48..].fill(128);
        let mut out = vec![0u8; 64 * 48 * 3 / 2];
        framer.compose_card(
            &video,
            CardFrame {
                rect: (4.0, 3.0, 56.0, 42.0),
                background_zoom: 1.0,
                pivot: (0.5, 0.5),
            },
            &outer,
            &converter,
            &mut out,
        );
        let background_luma = framer.background[0];
        // Outside the card the background shows.
        assert_eq!(out[2 * 64 + 2], background_luma);
        // The padding inside the card is footage.
        assert_eq!(out[5 * 64 + 6], 120);
        // The video rect still lands at (16, 12).
        assert_eq!(out[15 * 64 + 20], 200);
    }

    #[test]
    fn the_background_zooms_about_its_pivot() {
        let converter = Nv12Converter::new(YuvSpace::from_probe(None, None));
        let size = (64u32, 48u32);
        let mut background = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for (index, pixel) in background.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = index % size.0 as usize;
            *pixel = if x < 32 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            };
        }
        let canvas = CanvasLayout {
            size,
            video: (32, 24),
            radius: 0.0,
            area: (0, 0, size.0, size.1),
            background: Arc::new(background),
        };
        let mut framer = Framer::new(&canvas, size, &converter).unwrap();
        let video = vec![0u8; 32 * 24 * 3 / 2];
        let outer = vec![0u8; 64 * 48 * 3 / 2];
        let card = framer.rest();
        let mut out = vec![0u8; 64 * 48 * 3 / 2];
        framer.compose_card(
            &video,
            CardFrame {
                rect: card,
                background_zoom: 1.0,
                pivot: (0.5, 0.5),
            },
            &outer,
            &converter,
            &mut out,
        );
        assert!(
            out[2 * 64 + 60] > 200,
            "the right half should be white: {}",
            out[2 * 64 + 60]
        );
        framer.compose_card(
            &video,
            CardFrame {
                rect: card,
                background_zoom: 2.0,
                pivot: (0.0, 0.5),
            },
            &outer,
            &converter,
            &mut out,
        );
        assert!(
            out[2 * 64 + 60] < 50,
            "the zoom should bring the black half over it: {}",
            out[2 * 64 + 60]
        );
    }

    #[test]
    fn a_zoomed_card_rounds_its_own_corners() {
        let converter = Nv12Converter::new(YuvSpace::from_probe(None, None));
        let canvas = layout((64, 48), (32, 24), 6.0);
        let mut framer = Framer::new(&canvas, (64, 48), &converter).unwrap();
        let video = vec![200u8; 32 * 24 * 3 / 2];
        let mut outer = vec![0u8; 64 * 48 * 3 / 2];
        outer[..64 * 48].fill(120);
        outer[64 * 48..].fill(128);
        let mut out = vec![0u8; 64 * 48 * 3 / 2];
        framer.compose_card(
            &video,
            CardFrame {
                rect: (4.0, 3.0, 56.0, 42.0),
                background_zoom: 1.0,
                pivot: (0.5, 0.5),
            },
            &outer,
            &converter,
            &mut out,
        );
        let background_luma = framer.background[0];
        assert_eq!(out[3 * 64 + 4], background_luma);
        assert_eq!(out[3 * 64 + 32], 120);
    }

    #[test]
    fn rounded_coverage_is_full_inside_and_empty_outside() {
        let rect = (0.0, 0.0, 10.0, 10.0);
        assert_eq!(rounded_coverage(5, 5, rect, 3.0), 255);
        assert_eq!(rounded_coverage(0, 0, rect, 3.0), 0);
        assert_eq!(rounded_coverage(5, 0, rect, 3.0), 255);
    }
}
