//! The output canvas around the video: the background fill, the padding that
//! insets the video, a fixed frame aspect, and the video card's rounded
//! corners.
//!
//! The render paths produce the video rect in NV12 exactly as they would
//! without a canvas. [`FramedSink`] then lays it onto a background converted to
//! NV12 once per range: the video rows are copied into the centre and only the
//! anti-aliased corner pixels are blended. Passthrough and the GPU crop keep
//! working with a background, because neither ever sees the canvas.

use super::{crop_scale_into, End, Sink};
use crate::video_editor::pixels::Nv12Converter;
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
    /// The fill as opaque RGBA at `size`, row-major with no padding.
    pub background: Arc<Vec<u8>>,
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

/// Lays each video frame onto the canvas.
pub(super) struct Framer {
    out: (usize, usize),
    video: (usize, usize),
    /// Top-left of the video rect, even on both axes so chroma lines up.
    origin: (usize, usize),
    background: Vec<u8>,
    radius: usize,
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
        let scale = f64::from(out.0) / f64::from(layout.size.0.max(1));
        let radius = if layout.radius.is_finite() && layout.radius > 0.0 {
            ((layout.radius * scale).round().max(0.0) as usize)
                .min(video.0 as usize / 2)
                .min(video.1 as usize / 2)
        } else {
            0
        };
        Ok(Self {
            out: (out.0 as usize, out.1 as usize),
            video: (video.0 as usize, video.1 as usize),
            origin: (
                ((out.0 - video.0) / 2) as usize & !1,
                ((out.1 - video.1) / 2) as usize & !1,
            ),
            background,
            radius,
            corner: corner_coverage(radius),
        })
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
    pending: u64,
    spare: Vec<Vec<u8>>,
}

impl FramedSink {
    pub(super) fn new(sink: Sink, framer: Option<Framer>) -> Self {
        Self {
            sink,
            framer,
            pending: 0,
            spare: Vec::new(),
        }
    }

    pub(super) fn buffer(&mut self, index: u64) -> Result<Vec<u8>, End> {
        let Some(framer) = self.framer.as_ref() else {
            return self.sink.buffer(index);
        };
        self.pending = index;
        let bytes = framer.video_bytes();
        Ok(self
            .spare
            .pop()
            .filter(|buffer| buffer.len() == bytes)
            .unwrap_or_else(|| vec![0u8; bytes]))
    }

    pub(super) fn submit(&mut self, frame: Vec<u8>) -> Result<(), End> {
        let Some(framer) = self.framer.as_ref() else {
            return self.sink.submit(frame);
        };
        let mut canvas = self.sink.buffer(self.pending)?;
        framer.compose(&frame, &mut canvas);
        self.spare.push(frame);
        self.sink.submit(canvas)
    }

    pub(super) fn finish(self) -> Result<(), End> {
        self.sink.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{corner_coverage, video_size, CanvasLayout, Framer};
    use crate::video_editor::pixels::{Nv12Converter, YuvSpace};
    use std::sync::Arc;

    fn layout(size: (u32, u32), video: (u32, u32), radius: f64) -> CanvasLayout {
        CanvasLayout {
            size,
            video,
            radius,
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
}
