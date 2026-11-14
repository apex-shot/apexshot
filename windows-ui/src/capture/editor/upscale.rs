//! Offline neural enlargement for still-image exports.
//!
//! The two QuickSRNetMedium networks described in `assets/upscaling/README.md`
//! are embedded and run on the CPU through the same pure-Rust ONNX engine the
//! camera cutout uses, so enhancement needs no download or inference DLL.
//! Inference is CPU-bound; keep `enhance` off the GTK main thread where the
//! caller can.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use image::{imageops, RgbaImage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tract_onnx::prelude::*;

const TILE_SIZE: usize = 128;
const TILE_BORDER: usize = 8;
const TILE_CONTENT: usize = TILE_SIZE - TILE_BORDER * 2;
const MAX_INPUT_PIXELS: u64 = 1_048_576;
const MAX_INPUT_EDGE: u32 = 4096;
const MODEL_2X_BYTES: &[u8] = include_bytes!("../../../../assets/upscaling/model-2x.onnx");
const MODEL_4X_BYTES: &[u8] = include_bytes!("../../../../assets/upscaling/model-4x.onnx");

type Model = Arc<TypedRunnableModel>;

static MODEL_2X: OnceLock<Result<Model, String>> = OnceLock::new();
static MODEL_4X: OnceLock<Result<Model, String>> = OnceLock::new();
/// The most recent enhancement per mode, so a comparison preview and the
/// export that follows it run the network once.
static RECENT: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

struct Cached {
    key: u64,
    mode: SourceUpscale,
    image: Arc<RgbaImage>,
}

/// One tile's finished output pixels, in output coordinates.
struct Tile {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

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
    /// The exported image's enlargement factor.
    pub fn factor(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::Neural2x => 2,
            Self::Neural4x => 4,
        }
    }
}

fn load_model(bytes: &[u8]) -> Result<Model, String> {
    let mut reader = std::io::Cursor::new(bytes);
    let model = tract_onnx::onnx()
        .model_for_read(&mut reader)
        .map_err(|error| error.to_string())?;
    let model = model
        .into_optimized()
        .map_err(|error| error.to_string())?;
    model.into_runnable().map_err(|error| error.to_string())
}

pub fn enhance(image: &RgbaImage, mode: SourceUpscale) -> Result<RgbaImage, String> {
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
        .get_or_init(|| load_model(bytes))
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
    let origins: Vec<(usize, usize)> = (0..height as usize)
        .step_by(TILE_CONTENT)
        .flat_map(|top| {
            (0..width as usize)
                .step_by(TILE_CONTENT)
                .map(move |left| (left, top))
        })
        .collect();
    let tiles: Vec<Result<Tile, String>> = {
        let alpha = &enhanced;
        origins
            .par_iter()
            .map(|&(left, top)| run_tile(model, image, alpha, left, top, scale))
            .collect()
    };
    let out_width = enhanced.width() as usize;
    let buffer: &mut [u8] = &mut enhanced;
    for tile in tiles {
        let tile = tile?;
        for y in 0..tile.height {
            let start = ((tile.top + y) * out_width + tile.left) * 4;
            buffer[start..start + tile.width * 4]
                .copy_from_slice(&tile.pixels[y * tile.width * 4..(y + 1) * tile.width * 4]);
        }
    }
    Ok(enhanced)
}

fn run_tile(
    model: &Model,
    image: &RgbaImage,
    alpha: &RgbaImage,
    left: usize,
    top: usize,
    scale: usize,
) -> Result<Tile, String> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let mut input = vec![0.0f32; 3 * TILE_SIZE * TILE_SIZE];
    for y in 0..TILE_SIZE {
        let source_y = (top + y).saturating_sub(TILE_BORDER).min(height - 1);
        for x in 0..TILE_SIZE {
            let source_x = (left + x).saturating_sub(TILE_BORDER).min(width - 1);
            let pixel = image.get_pixel(source_x as u32, source_y as u32);
            for channel in 0..3 {
                input[(channel * TILE_SIZE + y) * TILE_SIZE + x] =
                    f32::from(pixel[channel]) * f32::from(pixel[3]) / (255.0 * 255.0);
            }
        }
    }
    let tensor = Tensor::from_shape(&[1, 3, TILE_SIZE, TILE_SIZE], &input)
        .map_err(|error| format!("Neural upscaling failed: {error}"))?;
    let outputs = model
        .run(tvec!(tensor.into()))
        .map_err(|error| format!("Neural upscaling failed: {error}"))?;
    let output = outputs
        .first()
        .ok_or_else(|| "Neural upscaling returned no image.".to_string())?;
    let view = output
        .to_plain_array_view::<f32>()
        .map_err(|error| format!("Unexpected neural upscaling output: {error}"))?;
    let edge = TILE_SIZE * scale;
    if view.shape() != [1, 3, edge, edge] {
        return Err("The neural upscaling model returned an unexpected image size.".into());
    }
    let data = view
        .as_slice()
        .ok_or_else(|| "Unexpected neural upscaling output layout.".to_string())?;
    let tile_width = TILE_CONTENT.min(width - left) * scale;
    let tile_height = TILE_CONTENT.min(height - top) * scale;
    let mut pixels = vec![0u8; tile_width * tile_height * 4];
    for y in 0..tile_height {
        for x in 0..tile_width {
            let a = alpha.get_pixel((left * scale + x) as u32, (top * scale + y) as u32)[3];
            if a == 0 {
                continue;
            }
            let index = (y * tile_width + x) * 4;
            pixels[index + 3] = a;
            for channel in 0..3 {
                let value = data
                    [(channel * edge + y + TILE_BORDER * scale) * edge + x + TILE_BORDER * scale];
                if !value.is_finite() {
                    return Err("The neural upscaling model returned invalid pixels.".into());
                }
                pixels[index + channel] = (value * 255.0 / f32::from(a))
                    .clamp(0.0, 1.0)
                    .mul_add(255.0, 0.5) as u8;
            }
        }
    }
    Ok(Tile {
        left: left * scale,
        top: top * scale,
        width: tile_width,
        height: tile_height,
        pixels,
    })
}

/// `enhance`, reusing the last result for the same pixels and mode.
pub fn enhance_cached(image: &RgbaImage, mode: SourceUpscale) -> Result<Arc<RgbaImage>, String> {
    if mode == SourceUpscale::Off {
        return Ok(Arc::new(image.clone()));
    }
    let key = fingerprint(image);
    if let Ok(recent) = RECENT.lock() {
        if let Some(entry) = recent
            .iter()
            .find(|entry| entry.key == key && entry.mode == mode)
        {
            return Ok(Arc::clone(&entry.image));
        }
    }
    let enhanced = Arc::new(enhance(image, mode)?);
    if let Ok(mut recent) = RECENT.lock() {
        recent.retain(|entry| entry.mode != mode);
        recent.push(Cached {
            key,
            mode,
            image: Arc::clone(&enhanced),
        });
    }
    Ok(enhanced)
}

fn fingerprint(image: &RgbaImage) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    image.dimensions().hash(&mut hasher);
    image.as_raw().hash(&mut hasher);
    hasher.finish()
}

/// The conventional enlargement a comparison sets beside the neural one.
pub fn standard_enlarge(image: &RgbaImage, mode: SourceUpscale) -> RgbaImage {
    if mode == SourceUpscale::Off {
        return image.clone();
    }
    let factor = mode.factor();
    imageops::resize(
        image,
        image.width() * factor,
        image.height() * factor,
        imageops::FilterType::CatmullRom,
    )
}

pub fn validate_input(image: &RgbaImage, mode: SourceUpscale) -> Result<(), String> {
    validate_input_dimensions(image.width(), image.height(), mode)
}

pub fn validate_input_dimensions(
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

    #[test]
    fn cached_enhancement_reuses_the_same_pixels() {
        let mut image = RgbaImage::from_pixel(20, 12, image::Rgba([30, 60, 90, 255]));
        let first = enhance_cached(&image, SourceUpscale::Neural2x).unwrap();
        let second = enhance_cached(&image, SourceUpscale::Neural2x).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.dimensions(), (40, 24));

        image.put_pixel(0, 0, image::Rgba([31, 60, 90, 255]));
        let changed = enhance_cached(&image, SourceUpscale::Neural2x).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
    }

    #[test]
    fn standard_enlargement_matches_the_neural_size() {
        let image = RgbaImage::from_pixel(9, 7, image::Rgba([120, 60, 30, 255]));
        for mode in [SourceUpscale::Neural2x, SourceUpscale::Neural4x] {
            assert_eq!(
                standard_enlarge(&image, mode).dimensions(),
                enhance(&image, mode).unwrap().dimensions()
            );
        }
    }

    #[test]
    #[ignore = "timing probe: cargo test megapixel -- --ignored --nocapture"]
    fn megapixel_enhancement_timing() {
        let image = RgbaImage::from_fn(1024, 1024, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
        });
        for mode in [SourceUpscale::Neural2x, SourceUpscale::Neural4x] {
            let start = std::time::Instant::now();
            let output = enhance(&image, mode).unwrap();
            eprintln!("{mode:?}: {:?} for {:?}", start.elapsed(), output.dimensions());
        }
    }
}
