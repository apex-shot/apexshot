//! Portable current-frame sampling for the live footage ripple.
//!
//! Clicks and the visible view stay in encoded source coordinates, so cropping,
//! camera zoom, and widget resizing do not change the wave's geometry. All
//! signed pulls are summed before sampling, matching the export displacement.

use super::click_effect::{PreparedRipple, RIPPLE_PULL_SIZE_01, RIPPLE_VISIBLE_DURATION_MS};
use rayon::prelude::*;

/// One click in encoded source pixels, aged in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewRippleClick {
    /// Horizontal source coordinate.
    pub(crate) x: f64,
    /// Vertical source coordinate.
    pub(crate) y: f64,
    /// Elapsed source time since the click.
    pub(crate) age_ms: f64,
}

/// A current-frame warp in the original video's coordinate system.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreviewRipple {
    /// Visible source rectangle as `(x, y, width, height)`.
    pub(crate) view: (f64, f64, f64, f64),
    /// Full encoded video width, before cropping or zooming.
    pub(crate) source_width: f64,
    /// Full encoded video height, before cropping or zooming.
    pub(crate) source_height: f64,
    /// Clicks in chronological order; only the last three valid visible clicks apply.
    pub(crate) clicks: Vec<PreviewRippleClick>,
}

impl PreviewRipple {
    /// Warp a tightly packed, four-component premultiplied current-frame texture.
    ///
    /// Component order is preserved, including alpha. `capture_bounds` is the
    /// captured rectangle `(x, y, width, height)` in widget-native pixels;
    /// `output_size` is that widget's native pixel size. The texture can include
    /// margins beyond the visible view. Samples are clamped to the original
    /// video's edges and then to the available texture, never to the crop.
    ///
    /// Bilinear interpolation reads only this frame, with no ring overlay or
    /// temporal averaging. Invalid geometry, buffer lengths, or unallocatable
    /// output sizes return an empty buffer. Invalid clicks are ignored.
    pub(crate) fn warp_pixels(
        &self,
        pixels: &[u8],
        image_size: (u32, u32),
        capture_bounds: (f64, f64, f64, f64),
        output_size: (u32, u32),
    ) -> Vec<u8> {
        let buffer_len = |size: (u32, u32)| {
            (size.0 as usize)
                .checked_mul(size.1 as usize)?
                .checked_mul(4)
                .filter(|&len| len > 0 && len <= isize::MAX as usize)
        };
        let Some(input_len) = buffer_len(image_size) else {
            return Vec::new();
        };
        let Some(output_len) = buffer_len(output_size) else {
            return Vec::new();
        };
        let (view_x, view_y, view_width, view_height) = self.view;
        let (capture_x, capture_y, capture_width, capture_height) = capture_bounds;
        if pixels.len() != input_len
            || ![
                view_x,
                view_y,
                capture_x,
                capture_y,
                view_x + view_width,
                view_y + view_height,
                capture_x + capture_width,
                capture_y + capture_height,
            ]
            .iter()
            .all(|value| value.is_finite())
            || ![
                view_width,
                view_height,
                capture_width,
                capture_height,
                self.source_width,
                self.source_height,
            ]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            return Vec::new();
        }
        let step_x = view_width / output_size.0 as f64;
        let step_y = view_height / output_size.1 as f64;
        let texture_scale_x = image_size.0 as f64 / capture_width;
        let texture_scale_y = image_size.1 as f64 / capture_height;
        let source_to_texture_x = texture_scale_x / step_x;
        let source_to_texture_y = texture_scale_y / step_y;
        let texture_x = |source_x: f64| {
            (source_x - view_x) * source_to_texture_x - capture_x * texture_scale_x - 0.5
        };
        let texture_y = |source_y: f64| {
            (source_y - view_y) * source_to_texture_y - capture_y * texture_scale_y - 0.5
        };
        if ![step_x, step_y, source_to_texture_x, source_to_texture_y]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
            || ![
                texture_x(0.0),
                texture_x(self.source_width),
                texture_y(0.0),
                texture_y(self.source_height),
            ]
            .iter()
            .all(|value| value.is_finite())
        {
            return Vec::new();
        }
        let limits = |start: f64, end: f64, size: u32| {
            let last = (size - 1) as f64;
            let lower = (start + 0.5).clamp(0.0, last);
            let upper = (end - 0.5).clamp(0.0, last);
            if lower <= upper {
                (lower, upper)
            } else {
                let centre = ((start + end) * 0.5).clamp(0.0, last);
                (centre, centre)
            }
        };
        let (min_x, max_x) = limits(texture_x(0.0), texture_x(self.source_width), image_size.0);
        let (min_y, max_y) = limits(texture_y(0.0), texture_y(self.source_height), image_size.1);
        let mut waves: Vec<_> = self
            .clicks
            .iter()
            .rev()
            .filter(|click| {
                click.x.is_finite()
                    && click.y.is_finite()
                    && (0.0..RIPPLE_VISIBLE_DURATION_MS).contains(&click.age_ms)
            })
            .take(3)
            .filter(|click| click.age_ms > 0.0)
            .map(|click| {
                PreparedRipple::new(
                    click.x,
                    click.y,
                    click.age_ms,
                    RIPPLE_PULL_SIZE_01,
                    self.source_width,
                )
            })
            .collect();
        waves.reverse();
        if waves.is_empty()
            && image_size == output_size
            && capture_bounds == (0.0, 0.0, output_size.0 as f64, output_size.1 as f64)
            && view_x >= 0.0
            && view_y >= 0.0
            && view_x + view_width <= self.source_width
            && view_y + view_height <= self.source_height
        {
            return pixels.to_vec();
        }
        let mut output = Vec::new();
        if output.try_reserve_exact(output_len).is_err() {
            return output;
        }
        output.resize(output_len, 0);
        let row_len = output_size.0 as usize * 4;
        let aligned_capture = texture_scale_x == 1.0
            && texture_scale_y == 1.0
            && capture_x.fract() == 0.0
            && capture_y.fract() == 0.0;
        let width = output_size.0 as usize;
        let copy_start = (min_x + capture_x).ceil().clamp(0.0, width as f64) as usize;
        let copy_end = ((max_x + capture_x).floor() + 1.0).clamp(0.0, width as f64) as usize;
        let render_row = |(y, row): (usize, &mut [u8])| {
            let source_y = view_y + (y as f64 + 0.5) * step_y;
            let baseline_y = (y as f64 + 0.5 - capture_y) * texture_scale_y - 0.5;
            let render_span = |start: usize, span: &mut [u8]| {
                for (offset, pixel) in span.chunks_exact_mut(4).enumerate() {
                    let x = start + offset;
                    let source_x = view_x + (x as f64 + 0.5) * step_x;
                    let (mut pull_x, mut pull_y) = (0.0, 0.0);
                    for wave in &waves {
                        let pull = wave.pull_px(source_x, source_y);
                        pull_x += pull.0;
                        pull_y += pull.1;
                    }
                    let baseline_x = (x as f64 + 0.5 - capture_x) * texture_scale_x - 0.5;
                    let (sample_x, sample_y) = if aligned_capture {
                        (
                            baseline_x - pull_x * source_to_texture_x,
                            baseline_y - pull_y * source_to_texture_y,
                        )
                    } else {
                        (
                            texture_x((source_x - pull_x).clamp(0.0, self.source_width)),
                            texture_y((source_y - pull_y).clamp(0.0, self.source_height)),
                        )
                    };
                    let sample_x = sample_x.clamp(min_x, max_x);
                    let sample_y = sample_y.clamp(min_y, max_y);
                    pixel.copy_from_slice(&bilinear_pixel(
                        pixels,
                        image_size.0,
                        sample_x,
                        sample_y,
                    ));
                }
            };
            if !aligned_capture
                || copy_start >= copy_end
                || baseline_y < min_y
                || baseline_y > max_y
            {
                render_span(0, row);
                return;
            }
            let copy_span = |start: usize, span: &mut [u8]| {
                let x = (start as f64 - capture_x) as usize;
                let offset = (baseline_y as usize * image_size.0 as usize + x) * 4;
                span.copy_from_slice(&pixels[offset..offset + span.len()]);
            };
            let mut spans = [(0, copy_start), (copy_end, width), (0, 0), (0, 0), (0, 0)];
            let mut count = 2;
            for wave in &waves {
                let Some((left, right)) = wave.row_bounds(source_y) else {
                    continue;
                };
                let start =
                    (((left - view_x) / step_x).floor() - 1.0).clamp(0.0, width as f64) as usize;
                let end =
                    (((right - view_x) / step_x).ceil() + 1.0).clamp(0.0, width as f64) as usize;
                if start < end {
                    spans[count] = (start, end);
                    count += 1;
                }
            }
            spans[..count].sort_unstable_by_key(|span| span.0);
            let mut cursor = 0;
            for &(start, end) in &spans[..count] {
                if cursor < start {
                    copy_span(cursor, &mut row[cursor * 4..start * 4]);
                }
                let start = start.max(cursor);
                if start < end {
                    render_span(start, &mut row[start * 4..end * 4]);
                }
                cursor = cursor.max(end);
            }
            if cursor < width {
                copy_span(cursor, &mut row[cursor * 4..]);
            }
        };
        if output_len >= 256 * 256 * 4 && output_size.1 >= 32 && rayon::current_num_threads() > 1 {
            output
                .par_chunks_exact_mut(row_len)
                .enumerate()
                .for_each(render_row);
        } else {
            output
                .chunks_exact_mut(row_len)
                .enumerate()
                .for_each(render_row);
        }
        output
    }
}

fn bilinear_pixel(pixels: &[u8], width: u32, x: f64, y: f64) -> [u8; 4] {
    let width = width as usize;
    let height = pixels.len() / (width * 4);
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    if fx == 0.0 && fy == 0.0 {
        let offset = (y0 * width + x0) * 4;
        return pixels[offset..offset + 4].try_into().unwrap();
    }
    let offsets = [
        (y0 * width + x0) * 4,
        (y0 * width + x1) * 4,
        (y1 * width + x0) * 4,
        (y1 * width + x1) * 4,
    ];
    std::array::from_fn(|channel| {
        let top = pixels[offsets[0] + channel] as f64 * (1.0 - fx)
            + pixels[offsets[1] + channel] as f64 * fx;
        let bottom = pixels[offsets[2] + channel] as f64 * (1.0 - fx)
            + pixels[offsets[3] + channel] as f64 * fx;
        (top * (1.0 - fy) + bottom * fy + 0.5) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::click_effect::{ripple_pull_px, ripple_radius_px};

    fn ripple(width: u32, height: u32) -> PreviewRipple {
        PreviewRipple {
            view: (0.0, 0.0, width as f64, height as f64),
            source_width: width as f64,
            source_height: height as f64,
            clicks: Vec::new(),
        }
    }

    fn pattern(width: u32, height: u32) -> Vec<u8> {
        (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    let alpha = 64 + ((x * 7 + y * 11) % 192) as u8;
                    [
                        (x * 3 % (alpha as u32 + 1)) as u8,
                        (y * 5 % (alpha as u32 + 1)) as u8,
                        ((x + y) * 13 % (alpha as u32 + 1)) as u8,
                        alpha,
                    ]
                })
            })
            .collect()
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
        let offset = ((y * width + x) * 4) as usize;
        &pixels[offset..offset + 4]
    }

    fn export_sample(
        ripple: &PreviewRipple,
        pixels: &[u8],
        size: (u32, u32),
        source: (f64, f64),
    ) -> [u8; 4] {
        let mut pull = (0.0, 0.0);
        for click in &ripple.clicks {
            let delta = ripple_pull_px(
                source.0,
                source.1,
                click.x,
                click.y,
                click.age_ms,
                RIPPLE_PULL_SIZE_01,
                ripple.source_width,
            );
            pull.0 += delta.0;
            pull.1 += delta.1;
        }
        bilinear_pixel(
            pixels,
            size.0,
            (source.0 - pull.0 - 0.5).clamp(0.0, (size.0 - 1) as f64),
            (source.1 - pull.1 - 0.5).clamp(0.0, (size.1 - 1) as f64),
        )
    }

    #[test]
    fn identity_and_inactive_waves_preserve_every_component_exactly() {
        let size = (43, 27);
        let pixels = pattern(size.0, size.1);
        let bounds = (0.0, 0.0, size.0 as f64, size.1 as f64);
        let mut effect = ripple(size.0, size.1);
        assert_eq!(effect.warp_pixels(&pixels, size, bounds, size), pixels);
        for age_ms in [-1.0, 0.0, 1000.0, 1001.0, f64::NAN] {
            effect.clicks = vec![PreviewRippleClick {
                x: 20.0,
                y: 13.0,
                age_ms,
            }];
            assert_eq!(effect.warp_pixels(&pixels, size, bounds, size), pixels);
        }
    }

    #[test]
    fn padded_capture_without_waves_is_an_exact_crop() {
        let size = (53, 35);
        let output_size = (43, 27);
        let pixels = pattern(size.0, size.1);
        let effect = ripple(output_size.0, output_size.1);
        let output = effect.warp_pixels(&pixels, size, (-5.0, -4.0, 53.0, 35.0), output_size);
        for y in 0..output_size.1 {
            for x in 0..output_size.0 {
                assert_eq!(
                    pixel(&output, output_size.0, x, y),
                    pixel(&pixels, size.0, x + 5, y + 4)
                );
            }
        }
    }

    #[test]
    fn aligned_capture_preserves_exact_pixels_with_noninteger_camera_scaling() {
        let size = (85, 53);
        let output_size = (71, 43);
        let pixels = pattern(size.0, size.1);
        let effect = PreviewRipple {
            view: (149.25, 71.0, 1727.3, 992.2),
            source_width: 1920.0,
            source_height: 1080.0,
            clicks: vec![PreviewRippleClick {
                x: -10000.0,
                y: -10000.0,
                age_ms: 130.0,
            }],
        };
        let output = effect.warp_pixels(&pixels, size, (-7.0, -5.0, 85.0, 53.0), output_size);
        for y in 0..output_size.1 {
            for x in 0..output_size.0 {
                assert_eq!(
                    pixel(&output, output_size.0, x, y),
                    pixel(&pixels, size.0, x + 7, y + 5)
                );
            }
        }
    }

    #[test]
    fn aligned_span_unions_match_scalar_sampling_for_disjoint_and_overlapping_waves() {
        let size = (420, 256);
        let output_size = (400, 240);
        let pixels = pattern(size.0, size.1);
        let mut effect = ripple(output_size.0, output_size.1);
        for click in [
            PreviewRippleClick {
                x: 40.5,
                y: 80.5,
                age_ms: 130.0,
            },
            PreviewRippleClick {
                x: 360.5,
                y: 160.5,
                age_ms: 400.0,
            },
            PreviewRippleClick {
                x: 200.5,
                y: 120.5,
                age_ms: 900.0,
            },
        ] {
            effect.clicks.push(click);
            let output =
                effect.warp_pixels(&pixels, size, (-10.0, -8.0, 420.0, 256.0), output_size);
            for y in 0..output_size.1 {
                for x in 0..output_size.0 {
                    let mut pull = (0.0, 0.0);
                    for click in &effect.clicks {
                        let delta = ripple_pull_px(
                            x as f64 + 0.5,
                            y as f64 + 0.5,
                            click.x,
                            click.y,
                            click.age_ms,
                            RIPPLE_PULL_SIZE_01,
                            effect.source_width,
                        );
                        pull.0 += delta.0;
                        pull.1 += delta.1;
                    }
                    let expected = bilinear_pixel(
                        &pixels,
                        size.0,
                        (x as f64 + 10.0 - pull.0).clamp(10.0, 409.0),
                        (y as f64 + 8.0 - pull.1).clamp(8.0, 247.0),
                    );
                    assert_eq!(pixel(&output, output_size.0, x, y), expected);
                }
            }
        }
    }

    #[test]
    fn known_source_pixel_moves_by_the_export_pull() {
        let size = (200, 120);
        let pixels = pattern(size.0, size.1);
        let mut effect = ripple(size.0, size.1);
        let age_ms = 100.0;
        let radius = ripple_radius_px(age_ms, RIPPLE_PULL_SIZE_01, effect.source_width);
        effect.clicks = vec![PreviewRippleClick {
            x: 90.5 - radius,
            y: 60.5,
            age_ms,
        }];
        let output = effect.warp_pixels(&pixels, size, (0.0, 0.0, 200.0, 120.0), size);
        let expected = export_sample(&effect, &pixels, size, (90.5, 60.5));
        assert_eq!(pixel(&output, size.0, 90, 60), expected);
        assert_ne!(
            pixel(&output, size.0, 90, 60),
            pixel(&pixels, size.0, 90, 60)
        );
    }

    #[test]
    fn crop_origin_zoom_and_resizing_keep_source_space_displacement() {
        let source_size = (200, 120);
        let pixels = pattern(source_size.0, source_size.1);
        let mut effect = ripple(source_size.0, source_size.1);
        effect.clicks = vec![PreviewRippleClick {
            x: 80.5,
            y: 60.5,
            age_ms: 100.0,
        }];
        for (view, output_size) in [
            ((20.0, 10.0, 100.0, 80.0), (100, 80)),
            ((20.0, 10.0, 100.0, 80.0), (200, 160)),
            ((20.0, 10.0, 100.0, 80.0), (50, 40)),
            ((37.25, 19.5, 82.5, 61.0), (99, 73)),
        ] {
            effect.view = view;
            let sx = output_size.0 as f64 / view.2;
            let sy = output_size.1 as f64 / view.3;
            let bounds = (-view.0 * sx, -view.1 * sy, 200.0 * sx, 120.0 * sy);
            let output = effect.warp_pixels(&pixels, source_size, bounds, output_size);
            for y in 0..output_size.1 {
                for x in 0..output_size.0 {
                    let source = (
                        view.0 + (x as f64 + 0.5) / sx,
                        view.1 + (y as f64 + 0.5) / sy,
                    );
                    let expected = export_sample(&effect, &pixels, source_size, source);
                    assert_eq!(pixel(&output, output_size.0, x, y), expected);
                }
            }
        }
    }

    #[test]
    fn signed_clicks_sum_before_sampling_and_only_the_last_three_apply() {
        let size = (200, 120);
        let pixels = pattern(size.0, size.1);
        let mut effect = ripple(size.0, size.1);
        effect.clicks = [100.0, 150.0, 400.0, 500.0]
            .into_iter()
            .map(|age_ms| PreviewRippleClick {
                x: 90.5 - ripple_radius_px(age_ms, RIPPLE_PULL_SIZE_01, 200.0),
                y: 60.5,
                age_ms,
            })
            .collect();
        let bounds = (0.0, 0.0, 200.0, 120.0);
        let four = effect.warp_pixels(&pixels, size, bounds, size);
        effect.clicks.remove(0);
        let three = effect.warp_pixels(&pixels, size, bounds, size);
        assert_eq!(four, three);
        assert_eq!(
            pixel(&three, size.0, 90, 60),
            export_sample(&effect, &pixels, size, (90.5, 60.5))
        );
        let positive = ripple_pull_px(
            90.5,
            60.5,
            effect.clicks[0].x,
            60.5,
            150.0,
            RIPPLE_PULL_SIZE_01,
            200.0,
        )
        .0;
        let negative = ripple_pull_px(
            90.5,
            60.5,
            effect.clicks[1].x,
            60.5,
            400.0,
            RIPPLE_PULL_SIZE_01,
            200.0,
        )
        .0;
        assert!(positive > 0.0 && negative < 0.0);
        effect.clicks = vec![effect.clicks[0]];
        assert_ne!(three, effect.warp_pixels(&pixels, size, bounds, size));
    }

    #[test]
    fn pixels_outside_the_band_and_at_the_click_are_unchanged() {
        let size = (200, 120);
        let pixels = pattern(size.0, size.1);
        let mut effect = ripple(size.0, size.1);
        effect.clicks = vec![PreviewRippleClick {
            x: 20.5,
            y: 20.5,
            age_ms: 100.0,
        }];
        let output = effect.warp_pixels(&pixels, size, (0.0, 0.0, 200.0, 120.0), size);
        assert_eq!(
            pixel(&output, size.0, 20, 20),
            pixel(&pixels, size.0, 20, 20)
        );
        assert_eq!(
            pixel(&output, size.0, 199, 119),
            pixel(&pixels, size.0, 199, 119)
        );
    }

    #[test]
    fn video_edges_clamp_without_reading_transparent_capture_margins() {
        let size = (220, 140);
        let output_size = (200, 120);
        let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
        for y in 10..130 {
            for x in 10..210 {
                let offset = ((y * size.0 + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&[12, 34, 56, 96]);
            }
        }
        let mut effect = ripple(output_size.0, output_size.1);
        for (x, y) in [(24.5, 60.5), (175.5, 60.5), (100.5, 24.5), (100.5, 95.5)] {
            effect.clicks = vec![PreviewRippleClick {
                x,
                y,
                age_ms: 400.0,
            }];
            let output =
                effect.warp_pixels(&pixels, size, (-10.0, -10.0, 220.0, 140.0), output_size);
            assert!(output
                .chunks_exact(4)
                .all(|pixel| pixel == [12, 34, 56, 96]));
        }
    }

    #[test]
    fn bilinear_interpolation_preserves_component_order_and_premultiplied_alpha() {
        let effect = ripple(2, 2);
        let pixels = [
            0, 10, 20, 40, 20, 30, 40, 80, 40, 50, 60, 120, 60, 70, 80, 160,
        ];
        let output = effect.warp_pixels(&pixels, (2, 2), (0.0, 0.0, 1.0, 1.0), (1, 1));
        assert_eq!(output, [30, 40, 50, 100]);
    }

    #[test]
    fn invalid_geometry_buffers_and_clicks_are_safe() {
        let size = (20, 12);
        let pixels = pattern(size.0, size.1);
        let bounds = (0.0, 0.0, 20.0, 12.0);
        let mut effect = ripple(size.0, size.1);
        assert!(effect
            .warp_pixels(&pixels[..3], size, bounds, size)
            .is_empty());
        assert!(effect
            .warp_pixels(&pixels, (0, 12), bounds, size)
            .is_empty());
        assert!(effect.warp_pixels(&pixels, size, bounds, (0, 0)).is_empty());
        assert!(effect
            .warp_pixels(&pixels, size, bounds, (u32::MAX, u32::MAX))
            .is_empty());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            effect.source_width = bad;
            assert!(effect.warp_pixels(&pixels, size, bounds, size).is_empty());
            effect.source_width = 20.0;
            effect.view.2 = bad;
            assert!(effect.warp_pixels(&pixels, size, bounds, size).is_empty());
            effect.view.2 = 20.0;
            assert!(effect
                .warp_pixels(&pixels, size, (0.0, 0.0, bad, 12.0), size)
                .is_empty());
        }
        effect.view.0 = f64::NAN;
        assert!(effect.warp_pixels(&pixels, size, bounds, size).is_empty());
        effect.view.0 = 0.0;
        effect.clicks = vec![PreviewRippleClick {
            x: f64::NAN,
            y: 0.0,
            age_ms: 100.0,
        }];
        assert_eq!(effect.warp_pixels(&pixels, size, bounds, size), pixels);
    }

    #[test]
    fn row_parallel_sampling_matches_the_serial_path() {
        let size = (400, 240);
        let pixels = pattern(size.0, size.1);
        let mut effect = ripple(size.0, size.1);
        effect.clicks = vec![PreviewRippleClick {
            x: 200.5,
            y: 120.5,
            age_ms: 150.0,
        }];
        let render = |threads| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| effect.warp_pixels(&pixels, size, (0.0, 0.0, 400.0, 240.0), size))
        };
        assert_eq!(render(1), render(4));
    }
}
