//! NV12 and RGBA conversion for the export pipeline.
//!
//! The compositor works in RGBA because Cairo draws the cursor in that space,
//! and x264 wants YUV, so both ends of the frame loop convert. Doing it here
//! rather than in ffmpeg keeps both pipes in the small NV12 layout — 1.5 bytes
//! a pixel instead of 4 — which is where most of the handoff's time goes.
//! Both directions run over rows in parallel, in integer maths with lookup
//! tables for the scalings the matrix leaves over.

use rayon::prelude::*;

/// The luma/chroma matrix coefficients of the source, so the RGBA the decoder
/// produced is converted back with the same weights it was built from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum YuvMatrix {
    #[default]
    Bt601,
    Bt709,
    Bt2020,
}

/// Full-range video keeps 0–255 for luma; limited range (the default for
/// H.264) uses 16–235 with chroma in 16–240.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct YuvSpace {
    pub matrix: YuvMatrix,
    pub full_range: bool,
}

impl YuvSpace {
    /// Build a space from ffprobe's `color_space` / `color_range` strings.
    ///
    /// Unknown values fall back to the ffmpeg default for an untagged stream
    /// (BT.601 limited), which is what the decoder assumed when it produced
    /// the RGBA in the first place.
    pub(super) fn from_probe(color_space: Option<&str>, color_range: Option<&str>) -> Self {
        let matrix = match color_space.unwrap_or_default().trim() {
            "bt709" => YuvMatrix::Bt709,
            "bt2020nc" | "bt2020c" | "bt2020" => YuvMatrix::Bt2020,
            _ => YuvMatrix::Bt601,
        };
        let full_range = matches!(
            color_range.unwrap_or_default().trim(),
            "pc" | "jpeg" | "full"
        );
        Self { matrix, full_range }
    }
}

/// The inverse matrix, scaled by 1024: red and blue each take one chroma
/// term, green takes both.
struct Inverse {
    red: [i32; 2],
    green: [i32; 2],
    blue: [i32; 2],
}

const fn inverse(matrix: YuvMatrix) -> Inverse {
    match matrix {
        // R = Y + 1.402 V, G = Y - 0.344 U - 0.714 V, B = Y + 1.772 U
        YuvMatrix::Bt601 => Inverse {
            red: [0, 1436],
            green: [-352, -731],
            blue: [1815, 0],
        },
        YuvMatrix::Bt709 => Inverse {
            red: [0, 1613],
            green: [-192, -479],
            blue: [1900, 0],
        },
        YuvMatrix::Bt2020 => Inverse {
            red: [0, 1510],
            green: [-168, -585],
            blue: [1926, 0],
        },
    }
}

/// Coefficients scaled by 1024: luma sums to 1024, chroma sums to zero.
struct Coefficients {
    luma: [i32; 3],
    blue: [i32; 3],
    red: [i32; 3],
}

const fn coefficients(matrix: YuvMatrix) -> Coefficients {
    match matrix {
        YuvMatrix::Bt601 => Coefficients {
            luma: [306, 601, 117],
            blue: [-173, -339, 512],
            red: [512, -429, -83],
        },
        YuvMatrix::Bt709 => Coefficients {
            luma: [218, 732, 74],
            blue: [-117, -395, 512],
            red: [512, -465, -47],
        },
        YuvMatrix::Bt2020 => Coefficients {
            luma: [269, 694, 61],
            blue: [-143, -369, 512],
            red: [512, -471, -41],
        },
    }
}

/// Quarter-level lookups from the matrix sums onto the output range, so the
/// per-pixel cost stays one multiply per channel instead of one division.
///
/// The sums arrive as `value * 1024`, which is shifted down to four steps per
/// level: 1021 luma entries, and 1024 chroma entries indexed at an offset of
/// 512 for the signed range.
struct Scaling {
    luma: Vec<u8>,
    chroma: Vec<u8>,
}

const LUMA_ENTRIES: usize = 1021;
const CHROMA_ENTRIES: usize = 1024;
const CHROMA_CENTRE: i32 = 512;

impl Scaling {
    fn new(full_range: bool) -> Self {
        let luma = (0..LUMA_ENTRIES)
            .map(|value| {
                let value = value as i64;
                let scaled = if full_range {
                    (value * 255 + 510) / 1020
                } else {
                    16 + (219 * value + 510) / 1020
                };
                scaled.min(255) as u8
            })
            .collect();
        let chroma = (0..CHROMA_ENTRIES)
            .map(|index| {
                let offset = index as i32 - CHROMA_CENTRE;
                let rounding = if offset >= 0 { 128 } else { -128 };
                let scaled = if full_range {
                    128 + (offset + if offset >= 0 { 2 } else { -2 }) / 4
                } else {
                    128 + (56 * offset + rounding) / 255
                };
                scaled.clamp(0, 255) as u8
            })
            .collect();
        Self { luma, chroma }
    }
}

/// Display-scale lookups for the reverse direction: an NV12 sample expanded
/// back to the 1024-scaled luma and signed chroma the inverse matrix wants.
///
/// Both tables are indexed by the raw byte, so decoding a frame costs a load
/// per sample rather than a division.
struct Decoding {
    luma: [i32; 256],
    chroma: [i32; 256],
}

impl Decoding {
    fn new(full_range: bool) -> Self {
        let mut luma = [0i32; 256];
        let mut chroma = [0i32; 256];
        for value in 0..256usize {
            let sample = value as i32;
            let level = if full_range {
                sample
            } else {
                // Studio-range luma, 16–235, expands back onto 0–255.
                ((sample - 16) * 255 + 109) / 219
            };
            luma[value] = level.clamp(0, 255) * 1024;
            let offset = if full_range {
                sample - 128
            } else {
                // Studio-range chroma, 16–240, expands back onto −128–127.
                ((sample - 128) * 255 + if sample >= 128 { 112 } else { -112 }) / 224
            };
            chroma[value] = offset.clamp(-128, 127) * 1024;
        }
        Self { luma, chroma }
    }
}

/// Converts composited RGBA frames into the NV12 layout x264 ingests.
///
/// Build one per export: the lookup tables depend only on the space, and
/// rebuilding them per frame would put two allocations in the frame loop.
pub(super) struct Nv12Converter {
    coefficients: Coefficients,
    inverse: Inverse,
    scaling: Scaling,
    decoding: Decoding,
}

impl Nv12Converter {
    pub(super) fn new(space: YuvSpace) -> Self {
        Self {
            coefficients: coefficients(space.matrix),
            inverse: inverse(space.matrix),
            scaling: Scaling::new(space.full_range),
            decoding: Decoding::new(space.full_range),
        }
    }

    /// Convert one RGBA frame of `size` into NV12: a full luma plane followed
    /// by interleaved U/V pairs at half resolution on both axes.
    ///
    /// `out` must hold `width * height * 3 / 2` bytes. Both dimensions are even
    /// because the export sizes the frame that way for yuv420p.
    pub(super) fn convert_into(&self, rgba: &[u8], size: (u32, u32), out: &mut [u8]) {
        let width = size.0 as usize;
        let height = size.1 as usize;
        debug_assert_eq!(rgba.len(), width * height * 4);
        debug_assert_eq!(out.len(), width * height * 3 / 2);
        let (luma_plane, chroma_plane) = out.split_at_mut(width * height);
        let [kr, kg, kb] = self.coefficients.luma;
        let [ur, ug, ub] = self.coefficients.blue;
        let [vr, vg, vb] = self.coefficients.red;
        let scaling = &self.scaling;

        luma_plane
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                let source = &rgba[y * width * 4..(y + 1) * width * 4];
                for (x, pixel) in row.iter_mut().enumerate() {
                    let [r, g, b] = rgb_at(source, x);
                    let index = ((kr * r + kg * g + kb * b) >> 8) as usize;
                    *pixel = scaling.luma[index.min(LUMA_ENTRIES - 1)];
                }
            });

        // Each chroma sample covers a 2x2 block, so its row stride is the frame
        // width and every block contributes an interleaved U,V pair.
        chroma_plane
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(chroma_y, row)| {
                let top = chroma_y * 2;
                let top_source = &rgba[top * width * 4..(top + 1) * width * 4];
                let bottom_source = &rgba[(top + 1) * width * 4..(top + 2) * width * 4];
                for (chroma_x, pair) in row.chunks_mut(2).enumerate() {
                    let x = chroma_x * 2;
                    let [r0, g0, b0] = rgb_at(top_source, x);
                    let [r1, g1, b1] = rgb_at(top_source, x + 1);
                    let [r2, g2, b2] = rgb_at(bottom_source, x);
                    let [r3, g3, b3] = rgb_at(bottom_source, x + 1);
                    let r = (r0 + r1 + r2 + r3) >> 2;
                    let g = (g0 + g1 + g2 + g3) >> 2;
                    let b = (b0 + b1 + b2 + b3) >> 2;
                    let u = ((ur * r + ug * g + ub * b) >> 8) + CHROMA_CENTRE;
                    let v = ((vr * r + vg * g + vb * b) >> 8) + CHROMA_CENTRE;
                    pair[0] = scaling.chroma[u.clamp(0, CHROMA_ENTRIES as i32 - 1) as usize];
                    pair[1] = scaling.chroma[v.clamp(0, CHROMA_ENTRIES as i32 - 1) as usize];
                }
            });
    }

    /// Expand one NV12 frame of `size` back into the RGBA the compositor draws
    /// on, upsampling chroma bilinearly the way swscale does.
    pub(super) fn to_rgba_into(&self, nv12: &[u8], size: (u32, u32), out: &mut [u8]) {
        // Chroma weights are 8-bit on purpose: a sample is at most 128 * 1024,
        // so 128 * 255 * 1024 still fits an i32 and the blend never needs a
        // 64-bit multiply.
        const ONE: i32 = 1 << 8;
        let width = size.0 as usize;
        let height = size.1 as usize;
        debug_assert_eq!(nv12.len(), width * height * 3 / 2);
        debug_assert_eq!(out.len(), width * height * 4);
        let (luma_plane, chroma_plane) = nv12.split_at(width * height);
        let chroma_width = (width / 2).max(1);
        let chroma_height = (height / 2).max(1);
        let [_, red_v] = self.inverse.red;
        let [green_u, green_v] = self.inverse.green;
        let [blue_u, _] = self.inverse.blue;
        let decoding = &self.decoding;
        let chroma_lut = &decoding.chroma;

        // A chroma sample sits between a pixel pair, so each output column
        // reads two neighbours. The taps hold for every row.
        let columns: Vec<(usize, usize, i32)> = (0..width)
            .map(|x| {
                let sample =
                    ((x as f64 - 0.5) * 0.5).clamp(0.0, chroma_width.saturating_sub(1) as f64);
                let x0 = sample as usize;
                (
                    x0,
                    (x0 + 1).min(chroma_width - 1),
                    (((sample - x0 as f64) * ONE as f64).round() as i32).clamp(0, ONE),
                )
            })
            .collect();

        out.par_chunks_mut(width * 4)
            .enumerate()
            .for_each(|(y, row)| {
                let sample =
                    ((y as f64 - 0.5) * 0.5).clamp(0.0, chroma_height.saturating_sub(1) as f64);
                let y0 = sample as usize;
                let y1 = (y0 + 1).min(chroma_height - 1);
                let fy = (((sample - y0 as f64) * ONE as f64).round() as i32).clamp(0, ONE);
                let inv_y = ONE - fy;
                let top = &chroma_plane[y0 * width..(y0 + 1) * width];
                let bottom = &chroma_plane[y1 * width..(y1 + 1) * width];
                let luma_row = &luma_plane[y * width..(y + 1) * width];
                let (pixels, _) = row.as_chunks_mut::<4>();
                for (x, pixel) in pixels.iter_mut().enumerate() {
                    let (x0, x1, fx) = columns[x];
                    let inv_x = ONE - fx;
                    let (left, right) = (x0 * 2, x1 * 2);
                    let u = blend(
                        chroma_lut,
                        (top[left], top[right], bottom[left], bottom[right]),
                        (inv_x, fx),
                        (inv_y, fy),
                    );
                    let v = blend(
                        chroma_lut,
                        (
                            top[left + 1],
                            top[right + 1],
                            bottom[left + 1],
                            bottom[right + 1],
                        ),
                        (inv_x, fx),
                        (inv_y, fy),
                    );
                    let luma = decoding.luma[luma_row[x] as usize];
                    pixel[0] = level(luma + ((red_v * v) >> 10));
                    pixel[1] = level(luma + ((green_u * u + green_v * v) >> 10));
                    pixel[2] = level(luma + ((blue_u * u) >> 10));
                    pixel[3] = 255;
                }
            });
    }
}

/// Interpolate one chroma plane's sample for a pixel from the four bytes
/// around it, in the 1024-scaled space the inverse matrix works in.
fn blend(
    lut: &[i32; 256],
    taps: (u8, u8, u8, u8),
    weights_x: (i32, i32),
    weights_y: (i32, i32),
) -> i32 {
    let (top_left, top_right, bottom_left, bottom_right) = taps;
    let (inv_x, fx) = weights_x;
    let (inv_y, fy) = weights_y;
    let top = (lut[top_left as usize] * inv_x + lut[top_right as usize] * fx + 128) >> 8;
    let bottom = (lut[bottom_left as usize] * inv_x + lut[bottom_right as usize] * fx + 128) >> 8;
    (top * inv_y + bottom * fy + 128) >> 8
}

/// Clamp a 1024-scaled sample back onto a byte.
fn level(value: i32) -> u8 {
    ((value + 512) >> 10).clamp(0, 255) as u8
}

fn rgb_at(source: &[u8], x: usize) -> [i32; 3] {
    let pixel = x * 4;
    [
        i32::from(source[pixel]),
        i32::from(source[pixel + 1]),
        i32::from(source[pixel + 2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::{Nv12Converter, YuvMatrix, YuvSpace};

    fn frame(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.iter().flatten().copied().collect()
    }

    fn convert(pixels: &[[u8; 4]], space: YuvSpace) -> (Vec<u8>, Vec<u8>) {
        // Every test frame is 2x2 so the chroma block is the whole image.
        let rgba = frame(pixels);
        let mut out = vec![0u8; 2 * 2 * 3 / 2];
        Nv12Converter::new(space).convert_into(&rgba, (2, 2), &mut out);
        (out[..4].to_vec(), out[4..].to_vec())
    }

    fn solid(value: [u8; 4]) -> Vec<[u8; 4]> {
        vec![value; 4]
    }

    #[test]
    #[ignore = "benchmark: times a full frame"]
    fn conversion_throughput_benchmark() {
        use std::time::Instant;
        for (width, height, label) in [(1920u32, 1080u32, "1080p"), (3840, 2160, "4K")] {
            let pixels = (width * height) as usize;
            let rgba: Vec<u8> = (0..pixels * 4).map(|index| (index % 251) as u8).collect();
            let mut nv12 = vec![0u8; pixels * 3 / 2];
            let mut back = vec![0u8; pixels * 4];
            let converter = Nv12Converter::new(YuvSpace::default());
            let frames = 20;
            let started = Instant::now();
            for _ in 0..frames {
                converter.convert_into(&rgba, (width, height), &mut nv12);
            }
            let encode = started.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
            let started = Instant::now();
            for _ in 0..frames {
                converter.to_rgba_into(&nv12, (width, height), &mut back);
            }
            let decode = started.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
            eprintln!(
                "[bench] {label} rgba->nv12 {encode:.2} ms/frame, nv12->rgba {decode:.2} ms/frame"
            );
        }
    }

    #[test]
    fn limited_range_black_and_white_land_on_the_video_scale() {
        let (luma, chroma) = convert(&solid([0, 0, 0, 255]), YuvSpace::default());
        assert_eq!(luma, vec![16, 16, 16, 16]);
        assert_eq!(chroma, vec![128, 128]);
        let (luma, chroma) = convert(&solid([255, 255, 255, 255]), YuvSpace::default());
        assert_eq!(luma, vec![235, 235, 235, 235]);
        assert_eq!(chroma, vec![128, 128]);
    }

    #[test]
    fn full_range_black_and_white_keep_the_display_scale() {
        let space = YuvSpace {
            matrix: YuvMatrix::Bt709,
            full_range: true,
        };
        let (luma, _) = convert(&solid([0, 0, 0, 255]), space);
        assert_eq!(luma, vec![0, 0, 0, 0]);
        let (luma, _) = convert(&solid([255, 255, 255, 255]), space);
        assert_eq!(luma, vec![255, 255, 255, 255]);
    }

    #[test]
    fn primary_colours_follow_the_matrix_they_are_told_to_use() {
        let red = solid([255, 0, 0, 255]);
        let (luma, chroma) = convert(&red, YuvSpace::default());
        // BT.601 limited: Y = 16 + 219 * 0.299, U = 128 - 224 * 0.169,
        // V = 128 + 224 * 0.5.
        assert_eq!(luma[0], 81);
        assert!((chroma[0] as i32 - 90).abs() <= 1, "U was {}", chroma[0]);
        assert_eq!(chroma[1], 240);

        let bt709 = YuvSpace {
            matrix: YuvMatrix::Bt709,
            full_range: false,
        };
        let (luma, _) = convert(&red, bt709);
        assert_eq!(luma[0], 63);
    }

    #[test]
    fn chroma_is_the_average_of_its_two_by_two_block() {
        // A left column of pure red and a right column of pure blue averages to
        // a magenta chroma, with per-pixel luma for the two columns.
        let pixels = [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
        ];
        let (luma, chroma) = convert(&pixels, YuvSpace::default());
        assert_eq!(luma, vec![81, 41, 81, 41]);
        let mixed_u = (90 + 240) / 2;
        assert!(
            (chroma[0] as i32 - mixed_u).abs() <= 2,
            "U was {}",
            chroma[0]
        );
    }

    #[test]
    fn the_alpha_channel_does_not_change_the_result() {
        let opaque = solid([12, 200, 90, 255]);
        let transparent = solid([12, 200, 90, 0]);
        assert_eq!(
            convert(&opaque, YuvSpace::default()),
            convert(&transparent, YuvSpace::default())
        );
    }

    fn round_trip(pixels: &[[u8; 4]], size: (u32, u32), space: YuvSpace) -> Vec<u8> {
        let rgba = frame(pixels);
        let mut nv12 = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let mut back = vec![0u8; rgba.len()];
        let converter = Nv12Converter::new(space);
        converter.convert_into(&rgba, size, &mut nv12);
        converter.to_rgba_into(&nv12, size, &mut back);
        back
    }

    #[test]
    fn a_solid_colour_survives_the_round_trip() {
        for colour in [
            [0u8, 0, 0],
            [255, 255, 255],
            [200, 30, 40],
            [10, 180, 240],
            [128, 128, 128],
        ] {
            let back = round_trip(
                &solid([colour[0], colour[1], colour[2], 255]),
                (2, 2),
                YuvSpace::default(),
            );
            for channel in 0..3 {
                let delta = back[channel].abs_diff(colour[channel]);
                assert!(
                    delta <= 3,
                    "{colour:?} came back as {back:?} (channel {channel} off by {delta})"
                );
            }
            assert!(back.iter().skip(3).step_by(4).all(|alpha| *alpha == 255));
        }
    }

    #[test]
    fn studio_range_luma_expands_back_to_the_display_scale() {
        // 2x2 with luma 16 (black) and 235 (white), neutral chroma.
        let mut nv12 = vec![0u8; 2 * 2 * 3 / 2];
        nv12[0] = 16;
        nv12[1] = 235;
        nv12[2] = 16;
        nv12[3] = 235;
        nv12[4] = 128;
        nv12[5] = 128;
        let mut rgba = vec![0u8; 2 * 2 * 4];
        Nv12Converter::new(YuvSpace::default()).to_rgba_into(&nv12, (2, 2), &mut rgba);
        assert_eq!(&rgba[..3], &[0, 0, 0]);
        assert_eq!(&rgba[4..7], &[255, 255, 255]);
    }

    #[test]
    fn chroma_is_upsampled_between_neighbouring_samples() {
        // A red left half against a blue right half: the pixels on the seam
        // read a chroma between the two blocks instead of jumping across.
        let width = 4u32;
        let height = 2u32;
        let pixels: Vec<[u8; 4]> = (0..(width * height))
            .map(|index| {
                if index % width < 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        let back = round_trip(&pixels, (width, height), YuvSpace::default());
        let blue = |x: usize| back[(x + width as usize) * 4 + 2];
        // With two chroma samples across four pixels, every column sits at a
        // different phase, so blue climbs instead of stepping.
        assert!(
            blue(0) < blue(1) && blue(1) < blue(2) && blue(2) < blue(3),
            "chroma did not blend across the seam: {} {} {} {}",
            blue(0),
            blue(1),
            blue(2),
            blue(3)
        );
        assert!(blue(0) < 90, "the left edge did not stay red: {}", blue(0));
        assert!(
            blue(3) > 165,
            "the right edge did not stay blue: {}",
            blue(3)
        );
    }

    #[test]
    fn a_frame_with_many_rows_converts_every_row() {
        // 6x4 exercises several chunk rows and three chroma rows.
        let width = 6usize;
        let height = 4usize;
        let mut rgba = vec![0u8; width * height * 4];
        let (pixels, _) = rgba.as_chunks_mut::<4>();
        for (index, pixel) in pixels.iter_mut().enumerate() {
            let x = index % width;
            let y = index / width;
            pixel.copy_from_slice(&[(x * 40) as u8, (y * 60) as u8, 128, 255]);
        }
        let mut out = vec![0u8; width * height * 3 / 2];
        Nv12Converter::new(YuvSpace::default()).convert_into(
            &rgba,
            (width as u32, height as u32),
            &mut out,
        );
        // The top-left 2x2 block must match converting those four pixels on
        // their own, and the rest of the frame must be written too.
        let block = [
            [0, 0, 128, 255],
            [40, 0, 128, 255],
            [0, 60, 128, 255],
            [40, 60, 128, 255],
        ];
        let (luma, chroma) = convert(&block, YuvSpace::default());
        assert_eq!(out[0], luma[0]);
        assert_eq!(&out[width * height..width * height + 2], &chroma[..2]);
        assert!(out[..width * height].iter().all(|value| *value != 0));
        assert!(out[width * height..].iter().any(|value| *value != 0));
    }
}
