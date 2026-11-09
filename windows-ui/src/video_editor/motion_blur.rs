use super::zoom::ViewRect;
use rayon::prelude::*;

/// The screen rectangle at the start and end of one camera trail, in output pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ScreenTrail {
    before: [f64; 4],
    now: [f64; 4],
}

impl ScreenTrail {
    pub(super) fn new(
        now: ViewRect,
        previous: ViewRect,
        source: (f64, f64),
        output: (u32, u32),
        fps: f64,
        amount: f64,
    ) -> Option<Self> {
        if !source.0.is_finite()
            || !source.1.is_finite()
            || !fps.is_finite()
            || fps <= 0.0
            || !amount.is_finite()
            || amount <= 0.0
            || source.0 <= 0.0
            || source.1 <= 0.0
            || output.0 == 0
            || output.1 == 0
        {
            return None;
        }
        let rect = |view: ViewRect| {
            let sx = f64::from(output.0) / view.2;
            let sy = f64::from(output.1) / view.3;
            [-view.0 * sx, -view.1 * sy, source.0 * sx, source.1 * sy]
        };
        let now = rect(now);
        let previous = rect(previous);
        let strength = amount.clamp(0.0, 1.0) * fps / 60.0;
        let before = std::array::from_fn(|i| now[i] + (previous[i] - now[i]) * strength);
        if now.iter().chain(before.iter()).any(|v| !v.is_finite())
            || before[2] <= 0.0
            || before[3] <= 0.0
            || now.iter().zip(before).all(|(a, b)| (a - b).abs() < 1.0)
        {
            return None;
        }
        Some(Self { before, now })
    }

    pub(super) fn in_camera_coordinates(
        now: ViewRect,
        previous: ViewRect,
        size: (u32, u32),
        fps: f64,
        amount: f64,
    ) -> Option<Self> {
        let previous = (
            (previous.0 - now.0) * f64::from(size.0) / now.2,
            (previous.1 - now.1) * f64::from(size.1) / now.3,
            previous.2 * f64::from(size.0) / now.2,
            previous.3 * f64::from(size.1) / now.3,
        );
        let source = (f64::from(size.0), f64::from(size.1));
        let full = (0.0, 0.0, source.0, source.1);
        let rect = |view: ViewRect| {
            let sx = source.0 / view.2;
            let sy = source.1 / view.3;
            [-view.0 * sx, -view.1 * sy, source.0 * sx, source.1 * sy]
        };
        let current = rect(full);
        let old = rect(previous);
        let strength = amount.clamp(0.0, 1.0) * fps / 60.0;
        let before = std::array::from_fn(|i| current[i] + (old[i] - current[i]) * strength);
        if !fps.is_finite()
            || fps <= 0.0
            || !amount.is_finite()
            || amount <= 0.0
            || current
                .iter()
                .chain(before.iter())
                .any(|value| !value.is_finite())
            || before[2] <= 0.0
            || before[3] <= 0.0
        {
            return None;
        }
        Some(Self {
            before,
            now: current,
        })
    }

    /// Render the current source frame along the trail. Screen footage is
    /// sampled in linear-light space, leaving the separate overlay layers alone.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        self,
        pixels: &[u8],
        size: (u32, u32),
        output_size: (u32, u32),
        output: &mut [u8],
        quality: usize,
        span: f64,
        anisotropy: usize,
    ) {
        let w = size.0 as usize;
        let h = size.1 as usize;
        let out_w = output_size.0 as usize;
        if w == 0
            || h == 0
            || out_w == 0
            || pixels.len() < w * h * 4
            || output.len() != out_w * output_size.1 as usize * 4
        {
            return;
        }
        let quality = quality.max(1);
        let span = span.max(1.0);
        let anisotropy = anisotropy.max(1);
        let mips = build_mips(pixels, w, h);
        // Source texels per output pixel.
        let minor = (w as f64 / self.before[2])
            .max(h as f64 / self.before[3])
            .max(1e-6);
        let columns: Vec<_> = (0..out_w)
            .map(|x| {
                let x = x as f64 + 0.5;
                (
                    (x - self.before[0]) / self.before[2] * w as f64 - 0.5,
                    (x - self.now[0]) / self.now[2] * w as f64 - 0.5,
                )
            })
            .collect();
        output
            .par_chunks_mut(out_w * 4)
            .enumerate()
            .for_each(|(y, row)| {
                let py = y as f64 + 0.5;
                let y0 = (py - self.before[1]) / self.before[3] * h as f64 - 0.5;
                let y1 = (py - self.now[1]) / self.now[3] * h as f64 - 0.5;
                for (x, &(x0, x1)) in columns.iter().enumerate() {
                    let dx = (x1 - x0) * self.before[2] / w as f64;
                    let dy = (y1 - y0) * self.before[3] / h as f64;
                    let distance = dx.hypot(dy);
                    let count = if distance < 1.0 {
                        1
                    } else {
                        ((distance / span).ceil() as usize).clamp(1, quality)
                    };
                    let phase = if distance < 1.0 { 0.5 } else { bayer8(x, y) };
                    let step = ((x1 - x0) / count as f64, (y1 - y0) / count as f64);
                    let major = step.0.hypot(step.1);
                    let taps = ((major / minor).ceil() as usize).clamp(1, anisotropy);
                    let lod = (major / taps as f64).max(minor).log2().max(0.0);
                    let mut sum = [0.0; 3];
                    for i in 0..count {
                        let t = (i as f64 + phase) / count as f64;
                        let cx = x0 + (x1 - x0) * t;
                        let cy = y0 + (y1 - y0) * t;
                        let mut average = [0.0; 3];
                        for k in 0..taps {
                            let o = (k as f64 + 0.5) / taps as f64 - 0.5;
                            let sx = (cx + step.0 * o).clamp(0.0, (w - 1) as f64);
                            let sy = (cy + step.1 * o).clamp(0.0, (h - 1) as f64);
                            let sample = trilinear(&mips, lod, sx, sy);
                            for c in 0..3 {
                                average[c] += sample[c];
                            }
                        }
                        for c in 0..3 {
                            let v = average[c] / taps as f64;
                            sum[c] += v * v;
                        }
                    }
                    for c in 0..3 {
                        row[x * 4 + c] =
                            (sum[c] / count as f64).sqrt().round().clamp(0.0, 255.0) as u8;
                    }
                    row[x * 4 + 3] = 255;
                }
            });
    }
}

/// Screen-space 8x8 Bayer threshold in [0, 1), as the studied build uses for its trail jitter.
fn bayer8(x: usize, y: usize) -> f64 {
    let (x, y) = (x % 8, y % 8);
    let bit = |v: usize, s: usize| (v >> s) & 1;
    let level = |s: usize| ((bit(x, s) ^ bit(y, s)) * 2 + bit(y, s)) as f64;
    (level(0) * 16.0 + level(1) * 4.0 + level(2)) / 64.0
}

struct Mip {
    w: usize,
    h: usize,
    rgb: Vec<f32>, // 3 f32 per texel, encoded 0..255 values (NOT squared)
}

fn build_mips(pixels: &[u8], w: usize, h: usize) -> Vec<Mip> {
    let mut rgb = Vec::with_capacity(w * h * 3);
    let (texels, _) = pixels.as_chunks::<4>();
    for texel in texels.iter().take(w * h) {
        rgb.push(texel[0] as f32);
        rgb.push(texel[1] as f32);
        rgb.push(texel[2] as f32);
    }
    let mut mips = vec![Mip { w, h, rgb }];
    while mips.len() < 8 {
        let parent = mips.last().unwrap();
        if parent.w == 1 && parent.h == 1 {
            break;
        }
        let nw = (parent.w / 2).max(1);
        let nh = (parent.h / 2).max(1);
        let mut rgb = Vec::with_capacity(nw * nh * 3);
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let px = (2 * x + dx).min(parent.w - 1);
                    let py = (2 * y + dy).min(parent.h - 1);
                    for (c, slot) in acc.iter_mut().enumerate() {
                        *slot += parent.rgb[(py * parent.w + px) * 3 + c];
                    }
                }
                for value in acc {
                    rgb.push(value / 4.0);
                }
            }
        }
        mips.push(Mip { w: nw, h: nh, rgb });
    }
    mips
}

fn bilinear(mip: &Mip, scale: f64, sx: f64, sy: f64) -> [f64; 3] {
    let lx = ((sx + 0.5) / scale - 0.5).clamp(0.0, (mip.w - 1) as f64);
    let ly = ((sy + 0.5) / scale - 0.5).clamp(0.0, (mip.h - 1) as f64);
    let ix = lx as usize;
    let iy = ly as usize;
    let fx = lx - ix as f64;
    let fy = ly - iy as f64;
    let ix1 = (ix + 1).min(mip.w - 1);
    let iy1 = (iy + 1).min(mip.h - 1);
    let mut out = [0.0; 3];
    for (c, slot) in out.iter_mut().enumerate() {
        let a = mip.rgb[(iy * mip.w + ix) * 3 + c] as f64;
        let b = mip.rgb[(iy * mip.w + ix1) * 3 + c] as f64;
        let d = mip.rgb[(iy1 * mip.w + ix) * 3 + c] as f64;
        let e = mip.rgb[(iy1 * mip.w + ix1) * 3 + c] as f64;
        *slot = (a + (b - a) * fx) * (1.0 - fy) + (d + (e - d) * fx) * fy;
    }
    out
}

fn trilinear(mips: &[Mip], lod: f64, sx: f64, sy: f64) -> [f64; 3] {
    let lod = lod.clamp(0.0, (mips.len() - 1) as f64);
    let l0 = lod.floor() as usize;
    let l1 = (l0 + 1).min(mips.len() - 1);
    let f = lod - l0 as f64;
    let a = bilinear(&mips[l0], (1u32 << l0) as f64, sx, sy);
    let b = bilinear(&mips[l1], (1u32 << l1) as f64, sx, sy);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * f)
}

#[cfg(test)]
mod tests {
    use super::ScreenTrail;

    #[test]
    fn trail_scales_previous_motion_with_fps_and_amount() {
        let now = (100.0, 0.0, 100.0, 100.0);
        let previous = (90.0, 0.0, 100.0, 100.0);
        let trail = ScreenTrail::new(now, previous, (200.0, 100.0), (200, 100), 60.0, 0.5).unwrap();
        assert!((trail.before[0] - trail.now[0] - 10.0).abs() < 1e-8);
        let half_fps =
            ScreenTrail::new(now, previous, (200.0, 100.0), (200, 100), 30.0, 0.5).unwrap();
        assert!((half_fps.before[0] - half_fps.now[0] - 5.0).abs() < 1e-8);
        assert!(ScreenTrail::new(now, previous, (200.0, 100.0), (200, 100), 60.0, 0.0).is_none());
        assert!(ScreenTrail::new(now, now, (200.0, 100.0), (200, 100), 60.0, 1.0).is_none());
    }

    #[test]
    fn equivalent_fps_and_strength_match_in_preview_and_export() {
        let now = (100.0, 40.0, 400.0, 200.0);
        let previous = (95.0, 36.0, 410.0, 205.0);
        let source = (800.0, 400.0);
        let size = (800, 400);
        for (fps, amount) in [(24.0, 1.0), (30.0, 0.5), (60.0, 1.0), (120.0, 0.5)] {
            let export = ScreenTrail::new(now, previous, source, size, fps, amount).unwrap();
            let preview =
                ScreenTrail::in_camera_coordinates(now, previous, size, fps, amount).unwrap();
            for (x, y) in [(100.0, 100.0), (400.0, 200.0), (700.0, 300.0)] {
                let actual = (
                    (x - export.before[0]) / export.before[2],
                    (y - export.before[1]) / export.before[3],
                );
                let u = (x - preview.before[0]) / preview.before[2];
                let v = (y - preview.before[1]) / preview.before[3];
                let expected = (
                    (now.0 + u * now.2) / source.0,
                    (now.1 + v * now.3) / source.1,
                );
                assert!(
                    (actual.0 - expected.0).abs() < 1e-9 && (actual.1 - expected.1).abs() < 1e-9,
                    "fps={fps} amount={amount} actual={actual:?} expected={expected:?}"
                );
            }
        }
    }

    #[test]
    fn moving_edge_is_averaged_in_linear_light() {
        let size = (32, 4);
        let mut pixels = vec![0; 32 * 4 * 4];
        for y in 0..4 {
            for x in 0..32 {
                let p = (y * 32 + x) * 4;
                let v = if x >= 16 { 255 } else { 0 };
                pixels[p..p + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let trail = ScreenTrail {
            before: [-8.0, 0.0, 32.0, 4.0],
            now: [0.0, 0.0, 32.0, 4.0],
        };
        let mut output = vec![0; pixels.len()];
        trail.render(&pixels, size, size, &mut output, 16, 1.0, 4);
        assert_eq!(output[4 * 3], 0);
        assert_eq!(output[4 * 24], 255);
        let edge = output[4 * 12];
        assert!(
            edge > 150 && edge < 220,
            "expected linear-light half coverage, got {edge}"
        );
    }

    #[test]
    fn bayer8_matches_the_studied_threshold_matrix() {
        assert_eq!(super::bayer8(0, 0), 0.0);
        assert_eq!(super::bayer8(1, 0), 0.5);
        assert_eq!(super::bayer8(0, 1), 0.75);
        assert_eq!(super::bayer8(1, 1), 0.25);
        let mut seen: Vec<u32> = (0..8)
            .flat_map(|y| (0..8).map(move |x| (super::bayer8(x, y) * 64.0) as u32))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn long_trail_filters_fine_detail_instead_of_ghosting() {
        // 1-texel black/white columns; a 32 px trail with 4 fetches puts every
        // fetch 8 texels apart, so point samples would all hit one colour.
        let size = (64, 4);
        let mut pixels = vec![0; 64 * 4 * 4];
        for y in 0..4 {
            for x in 0..64 {
                let p = (y * 64 + x) * 4;
                let v = if x % 2 == 0 { 255 } else { 0 };
                pixels[p..p + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let trail = ScreenTrail {
            before: [-32.0, 0.0, 64.0, 4.0],
            now: [0.0, 0.0, 64.0, 4.0],
        };
        let mut output = vec![0; pixels.len()];
        trail.render(&pixels, size, size, &mut output, 4, 1.0, 4);
        for x in 8..56 {
            let v = output[4 * x];
            assert!((100..=160).contains(&v), "column {x} ghosted to {v}");
        }
    }
}
