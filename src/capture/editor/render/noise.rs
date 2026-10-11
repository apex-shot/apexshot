//! Background grain ("noise").
//!
//! One implementation feeds the static canvas, the static export, and the
//! Motion compositor so a fill reads the same in every preview: a
//! deterministic speckle field overlay-blended into the background only,
//! preserving its average color rather than pulling it toward gray. The
//! captured card is composited on top afterwards, so the screenshot stays clean.

use gtk4::cairo::{Context, Extend, Filter, Format, ImageSurface, Operator, SurfacePattern};
use image::{Rgba, RgbaImage};
use std::cell::RefCell;

use super::{cairo_argb_to_rgba_image, rgba_image_to_surface};

/// Side of the repeating grain tile, in pixels. Large enough that the repeat
/// never reads as a pattern at a 100% slider.
const TILE_SIZE: u32 = 512;
/// Peak speckle opacity at a 100% slider: clearly grainy, while the color,
/// gradient, or wallpaper underneath still shows through. Higher values read
/// as television snow rather than film grain.
const MAX_SPECKLE_ALPHA: f64 = 0.24;
/// Faintest speckle, as a fraction of the slider's opacity. Varying the
/// opacity per pixel is what makes the field read as grain instead of a
/// checkerboard.
const MIN_SPECKLE_SCALE: f64 = 0.30;

thread_local! {
    /// The tile is rebuilt only when the slider moves; every draw after that is
    /// one repeating-pattern fill instead of thousands of rectangles.
    static NOISE_TILE: RefCell<Option<(u8, ImageSurface)>> = const { RefCell::new(None) };
}

/// Murmur-style finalizer, so neighbouring pixels get unrelated values.
fn hash32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value
}

/// Deterministic light/dark choice and opacity for one pixel, so the grain
/// never shimmers as the Motion playhead advances and preview matches export.
fn speckle(x: u32, y: u32) -> (bool, f64) {
    let hash = hash32(x.wrapping_mul(0x9e37_79b9) ^ hash32(y.wrapping_mul(0x85eb_ca6b)));
    let light = hash & 0x8000_0000 != 0;
    let scale = MIN_SPECKLE_SCALE
        + (1.0 - MIN_SPECKLE_SCALE) * f64::from(hash & 0xffff) / f64::from(u16::MAX);
    (light, scale)
}

/// Peak speckle opacity for a 0..1 slider. Zero means "no grain at all".
fn speckle_alpha(amount: f64) -> u8 {
    (amount.clamp(0.0, 1.0) * MAX_SPECKLE_ALPHA * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn noise_tile(max_alpha: u8) -> Option<ImageSurface> {
    if max_alpha == 0 {
        return None;
    }
    NOISE_TILE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached, surface)) = cache.as_ref() {
            if *cached == max_alpha {
                return Some(surface.clone());
            }
        }
        // Grain is per pixel. Anything coarser (equal-size blocks) reads as a
        // checker texture, and it goes soft as soon as the export is viewed
        // scaled down, while per-pixel grain keeps the detail at 100%.
        let mut tile = RgbaImage::new(TILE_SIZE, TILE_SIZE);
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let (light, scale) = speckle(x, y);
                let value = if light { 255 } else { 0 };
                let alpha = (f64::from(max_alpha) * scale).round() as u8;
                tile.put_pixel(x, y, Rgba([value, value, value, alpha]));
            }
        }
        let surface = rgba_image_to_surface(&tile)?;
        *cache = Some((max_alpha, surface.clone()));
        Some(surface)
    })
}

/// Paint grain over the rect at (`x`, `y`) sized (`width`, `height`) in device
/// pixels. Callers pass the background's own rect; any outer clip (the Motion
/// preview's scene panel) still applies on top of it.
pub fn paint_background_noise(
    context: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    amount: f64,
) {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return;
    }
    let Some(tile) = noise_tile(speckle_alpha(amount)) else {
        return;
    };
    let pattern = SurfacePattern::create(&tile);
    pattern.set_extend(Extend::Repeat);
    // Nearest keeps the cells crisp. The pattern is drawn unscaled, so bilinear
    // sampling would only soften the speckles it is meant to show.
    pattern.set_filter(Filter::Nearest);
    let _ = context.save();
    context.rectangle(x, y, width, height);
    context.clip();
    context.translate(x, y);
    context.set_operator(Operator::Overlay);
    let _ = context.set_source(&pattern);
    let _ = context.paint();
    let _ = context.restore();
}

/// Grain a composited canvas in `RgbaImage` space, for the static export that
/// composes pixels directly. Paints through a surface round trip so the result
/// is the same field the canvas preview shows.
pub fn apply_background_noise(canvas: RgbaImage, amount: f64) -> RgbaImage {
    if speckle_alpha(amount) == 0 || canvas.width() == 0 || canvas.height() == 0 {
        return canvas;
    }
    let Some(mut surface) = rgba_image_to_surface(&canvas) else {
        return canvas;
    };
    {
        let Ok(context) = Context::new(&surface) else {
            return canvas;
        };
        paint_background_noise(
            &context,
            0.0,
            0.0,
            f64::from(canvas.width()),
            f64::from(canvas.height()),
            amount,
        );
    }
    surface.flush();
    let Ok(stride) = Format::ARgb32.stride_for_width(canvas.width()) else {
        return canvas;
    };
    let Ok(data) = surface.data() else {
        return canvas;
    };
    cairo_argb_to_rgba_image(
        canvas.width(),
        canvas.height(),
        stride as usize,
        data.as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface_pixel(surface: &mut ImageSurface, x: usize, y: usize) -> [u8; 4] {
        let width = surface.width() as usize;
        let data = surface.data().unwrap();
        let offset = (y * width + x) * 4;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn speckle_alpha_tracks_the_slider_and_floors_at_zero() {
        assert_eq!(speckle_alpha(0.0), 0);
        assert_eq!(speckle_alpha(-3.0), 0);
        assert_eq!(speckle_alpha(4.0), speckle_alpha(1.0));
        assert!(speckle_alpha(0.25) > 0 && speckle_alpha(0.25) < speckle_alpha(1.0));
        assert!(
            f64::from(speckle_alpha(1.0)) <= (MAX_SPECKLE_ALPHA * 255.0).round(),
            "the maximum slider must stay inside the tuned speckle opacity",
        );
    }

    #[test]
    fn the_grain_field_is_deterministic_and_amount_scales_its_opacity() {
        let render = |amount: f64| {
            let mut surface = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
            {
                let context = Context::new(&surface).unwrap();
                context.set_source_rgb(0.5, 0.5, 0.5);
                context.paint().unwrap();
                paint_background_noise(&context, 0.0, 0.0, 64.0, 64.0, amount);
            }
            surface.flush();
            let bytes = surface.data().unwrap().to_vec();
            bytes
        };

        let quiet = render(0.3);
        let loud = render(1.0);

        assert_eq!(
            quiet,
            render(0.3),
            "the same amount must paint the same field on every frame",
        );
        assert_ne!(
            quiet, loud,
            "a louder slider must raise the speckle opacity",
        );
        assert!(noise_tile(0).is_none(), "zero noise needs no tile at all");
    }

    #[test]
    fn grain_lands_inside_the_rect_only() {
        let mut surface = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            context.set_source_rgb(0.2, 0.2, 0.2);
            context.paint().unwrap();
            paint_background_noise(&context, 8.0, 8.0, 32.0, 32.0, 1.0);
        }
        surface.flush();

        let base = surface_pixel(&mut surface, 0, 0);
        let grained = (8..40)
            .flat_map(|y| (8..40).map(move |x| (x, y)))
            .any(|(x, y)| surface_pixel(&mut surface, x, y) != base);
        assert!(grained, "the background rect must show visible grain");
        for y in [0, 4, 60, 63] {
            for x in [0, 4, 60, 63] {
                assert_eq!(
                    surface_pixel(&mut surface, x, y),
                    base,
                    "grain must not spill outside its rect at ({x}, {y})",
                );
            }
        }
    }

    #[test]
    fn zero_amount_paints_nothing() {
        let mut surface = ImageSurface::create(Format::ARgb32, 32, 32).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            context.set_source_rgb(0.2, 0.2, 0.2);
            context.paint().unwrap();
            paint_background_noise(&context, 0.0, 0.0, 32.0, 32.0, 0.0);
        }
        surface.flush();

        let base = surface_pixel(&mut surface, 0, 0);
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(surface_pixel(&mut surface, x, y), base);
            }
        }
    }

    #[test]
    fn image_grain_matches_the_painted_field() {
        let canvas = RgbaImage::from_pixel(48, 48, Rgba([60, 60, 60, 255]));
        let grained = apply_background_noise(canvas.clone(), 0.8);
        let untouched = apply_background_noise(canvas.clone(), 0.0);

        assert_eq!(untouched.as_raw(), canvas.as_raw(), "zero noise is a no-op");
        let differs = grained
            .pixels()
            .zip(canvas.pixels())
            .any(|(after, before)| after != before);
        assert!(differs, "the exported canvas must carry grain");
        assert_eq!(grained.get_pixel(0, 0)[3], 255, "grain stays opaque");
    }

    #[test]
    fn grain_preserves_dark_light_and_saturated_background_colors() {
        for rgb in [[18, 18, 18], [237, 237, 237], [240, 24, 64], [24, 56, 240]] {
            let canvas =
                RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgba([rgb[0], rgb[1], rgb[2], 255]));
            for amount in [0.3, 1.0] {
                let grained = apply_background_noise(canvas.clone(), amount);
                for (channel, base) in rgb.into_iter().enumerate() {
                    let mean = grained
                        .pixels()
                        .map(|pixel| f64::from(pixel[channel]))
                        .sum::<f64>()
                        / f64::from(TILE_SIZE * TILE_SIZE);
                    assert!(
                        (mean - f64::from(base)).abs() < 1.0,
                        "grain must preserve {rgb:?} at {amount}, channel {channel} averaged {mean}",
                    );
                    assert!(
                        grained.pixels().any(|pixel| pixel[channel] < base)
                            && grained.pixels().any(|pixel| pixel[channel] > base),
                        "grain must vary both sides of the background color",
                    );
                }
            }
        }
    }

    #[test]
    fn grain_does_not_turn_black_or_white_into_gray() {
        for value in [0, 255] {
            let canvas = RgbaImage::from_pixel(64, 64, Rgba([value, value, value, 255]));
            assert!(
                apply_background_noise(canvas, 1.0)
                    .pixels()
                    .all(|pixel| *pixel == Rgba([value, value, value, 255])),
                "grain must not add a gray cast to {value}",
            );
        }
    }

    #[test]
    fn painting_grain_restores_the_callers_operator() {
        let surface = ImageSurface::create(Format::ARgb32, 32, 32).unwrap();
        let context = Context::new(&surface).unwrap();
        context.set_operator(Operator::Source);
        paint_background_noise(&context, 0.0, 0.0, 32.0, 32.0, 1.0);
        assert_eq!(context.operator(), Operator::Source);
    }
}
