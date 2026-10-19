use image::{imageops, RgbaImage};
use rten_tensor::{prelude::*, NdTensor};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const TILE_SIZE: usize = 128;
const TILE_BORDER: usize = 8;
const TILE_CONTENT: usize = TILE_SIZE - TILE_BORDER * 2;
const MAX_INPUT_PIXELS: u64 = 1_048_576;
const MAX_INPUT_EDGE: u32 = 4096;
const MODEL_2X_BYTES: &[u8] = include_bytes!("../../../assets/upscaling/model-2x.onnx");
const MODEL_4X_BYTES: &[u8] = include_bytes!("../../../assets/upscaling/model-4x.onnx");
static MODEL_2X: OnceLock<Result<rten::Model, String>> = OnceLock::new();
static MODEL_4X: OnceLock<Result<rten::Model, String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Optional offline neural enhancement applied at export time.
pub enum SourceUpscale {
    #[default]
    Off,
    Neural2x,
    Neural4x,
}

impl SourceUpscale {
    /// The exported image or prepared Motion texture's enlargement factor.
    pub fn factor(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::Neural2x => 2,
            Self::Neural4x => 4,
        }
    }
}

pub(crate) fn enhance(image: &RgbaImage, mode: SourceUpscale) -> Result<RgbaImage, String> {
    if mode == SourceUpscale::Off {
        return Ok(image.clone());
    }
    validate_input(image, mode)?;
    let (width, height) = image.dimensions();
    let scale = mode.factor() as usize;
    let (cache, bytes) = if mode == SourceUpscale::Neural2x {
        (&MODEL_2X, MODEL_2X_BYTES)
    } else {
        (&MODEL_4X, MODEL_4X_BYTES)
    };
    let model = cache
        .get_or_init(|| rten::Model::load_static_slice(bytes).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| format!("Could not load the neural upscaling model: {error}"))?;
    let mut enhanced = if image.pixels().all(|pixel| pixel[3] == 255) {
        RgbaImage::from_pixel(
            width * scale as u32,
            height * scale as u32,
            image::Rgba([0, 0, 0, 255]),
        )
    } else {
        imageops::resize(
            image,
            width * scale as u32,
            height * scale as u32,
            imageops::FilterType::Lanczos3,
        )
    };
    for top in (0..height as usize).step_by(TILE_CONTENT) {
        for left in (0..width as usize).step_by(TILE_CONTENT) {
            let mut input = NdTensor::<f32, 4>::zeros([1, 3, TILE_SIZE, TILE_SIZE]);
            for y in 0..TILE_SIZE {
                let source_y = (top + y)
                    .saturating_sub(TILE_BORDER)
                    .min(height as usize - 1);
                for x in 0..TILE_SIZE {
                    let source_x = (left + x)
                        .saturating_sub(TILE_BORDER)
                        .min(width as usize - 1);
                    let pixel = image.get_pixel(source_x as u32, source_y as u32);
                    for channel in 0..3 {
                        input[[0, channel, y, x]] =
                            f32::from(pixel[channel]) * f32::from(pixel[3]) / (255.0 * 255.0);
                    }
                }
            }
            let output = model
                .run_one(input.view().into(), None)
                .map_err(|error| format!("Neural upscaling failed: {error}"))?;
            let output: NdTensor<f32, 4> = output
                .try_into()
                .map_err(|error| format!("Unexpected neural upscaling output: {error}"))?;
            if output.shape() != [1, 3, TILE_SIZE * scale, TILE_SIZE * scale] {
                return Err("The neural upscaling model returned an unexpected image size.".into());
            }
            let tile_width = TILE_CONTENT.min(width as usize - left) * scale;
            let tile_height = TILE_CONTENT.min(height as usize - top) * scale;
            for y in 0..tile_height {
                for x in 0..tile_width {
                    let pixel =
                        enhanced.get_pixel_mut((left * scale + x) as u32, (top * scale + y) as u32);
                    if pixel[3] == 0 {
                        pixel.0 = [0, 0, 0, 0];
                        continue;
                    }
                    for channel in 0..3 {
                        let value =
                            output[[0, channel, y + TILE_BORDER * scale, x + TILE_BORDER * scale]];
                        if !value.is_finite() {
                            return Err(
                                "The neural upscaling model returned invalid pixels.".into()
                            );
                        }
                        pixel[channel] = (value * 255.0 / f32::from(pixel[3]))
                            .clamp(0.0, 1.0)
                            .mul_add(255.0, 0.5) as u8;
                    }
                }
            }
        }
    }
    Ok(enhanced)
}

pub(crate) fn validate_input(image: &RgbaImage, mode: SourceUpscale) -> Result<(), String> {
    validate_input_dimensions(image.width(), image.height(), mode)
}

pub(crate) fn validate_input_dimensions(
    width: u32,
    height: u32,
    mode: SourceUpscale,
) -> Result<(), String> {
    if mode == SourceUpscale::Off {
        return Ok(());
    }
    if width == 0 || height == 0 {
        return Err("Cannot upscale an empty image.".into());
    }
    if u64::from(width) * u64::from(height) > MAX_INPUT_PIXELS || width.max(height) > MAX_INPUT_EDGE
    {
        return Err(
            "Neural upscaling supports images up to 1 megapixel and 4096 pixels per edge.".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_enhancement_preserves_every_pixel() {
        let image = RgbaImage::from_fn(9, 5, |x, y| image::Rgba([x as u8, y as u8, 100, 120]));
        assert_eq!(enhance(&image, SourceUpscale::Off).unwrap(), image);
    }

    #[test]
    fn neural_enhancement_preserves_native_scale_across_tiles() {
        let (width, height) = (TILE_CONTENT as u32 + 3, TILE_CONTENT as u32 + 5);
        let source = image::Rgba([180, 90, 45, 255]);
        let image = RgbaImage::from_pixel(width, height, source);
        for mode in [SourceUpscale::Neural2x, SourceUpscale::Neural4x] {
            let result = enhance(&image, mode).unwrap();
            assert_eq!(
                result.dimensions(),
                (width * mode.factor(), height * mode.factor())
            );
            for pixel in result.pixels() {
                assert_eq!(pixel[3], 255);
                for channel in 0..3 {
                    assert!((i16::from(pixel[channel]) - i16::from(source[channel])).abs() <= 3);
                }
            }
        }
    }

    #[test]
    fn partially_transparent_input_preserves_resized_alpha() {
        let image = RgbaImage::from_fn(9, 7, |x, y| {
            let alpha = ((x + y) % 4 * 64) as u8;
            image::Rgba([180, 90, 45, alpha])
        });

        for mode in [SourceUpscale::Neural2x, SourceUpscale::Neural4x] {
            let scale = mode.factor();
            let expected_alpha = imageops::resize(
                &image,
                image.width() * scale,
                image.height() * scale,
                imageops::FilterType::Lanczos3,
            );
            let result = enhance(&image, mode).unwrap();

            for (actual, expected) in result.pixels().zip(expected_alpha.pixels()) {
                assert_eq!(actual[3], expected[3]);
                if expected[3] == 0 {
                    assert_eq!(actual.0, [0, 0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn enhancement_rejects_empty_and_oversized_inputs_before_inference() {
        assert!(enhance(&RgbaImage::new(0, 0), SourceUpscale::Neural4x).is_err());
        assert!(enhance(&RgbaImage::new(1025, 1024), SourceUpscale::Neural2x).is_err());
        assert!(enhance(&RgbaImage::new(4097, 1), SourceUpscale::Neural2x).is_err());
    }

    #[test]
    fn input_limits_are_inclusive_at_the_pixel_and_edge_caps() {
        let accepted = RgbaImage::new(MAX_INPUT_EDGE, 256);
        assert_eq!(
            u64::from(accepted.width()) * u64::from(accepted.height()),
            MAX_INPUT_PIXELS
        );
        assert!(validate_input(&accepted, SourceUpscale::Neural4x).is_ok());
        assert!(validate_input(
            &RgbaImage::new(MAX_INPUT_EDGE, 257),
            SourceUpscale::Neural4x
        )
        .is_err());
    }

    #[test]
    fn transparent_pixels_remain_transparent() {
        let result = enhance(&RgbaImage::new(5, 3), SourceUpscale::Neural4x).unwrap();
        assert!(result.pixels().all(|pixel| pixel.0 == [0, 0, 0, 0]));
    }
}
