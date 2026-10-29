//! Person segmentation for the camera cutout.
//!
//! The camera layer needs a per-pixel person mask rather than a keyed colour.
//! This module embeds two small Apache-2.0 segmentation networks in the
//! Windows binary and runs them on the CPU through a pure-Rust ONNX engine, so
//! the editor has no runtime download, Python, or external inference DLL.
//!
//! Wide frames use the landscape 256x144 network; square and portrait frames
//! use the general 256x256 one. Each frame is letterboxed into its network with
//! zero bars, so the subject keeps its proportions; stretching a square track
//! into the landscape canvas squashes the subject and degrades the mask, and
//! stretching a wide track into the square canvas distorts it. The sigmoid mask
//! is resized back from the letterboxed content area. Both resizes and the
//! inference are CPU-bound; callers must keep `matte` off the GTK main thread.

use tract_onnx::prelude::*;

const LANDSCAPE_MODEL_BYTES: &[u8] =
    include_bytes!("../../assets/camera-cutout/selfie_segmentation_landscape.onnx");
const GENERAL_MODEL_BYTES: &[u8] =
    include_bytes!("../../assets/camera-cutout/selfie_segmentation.onnx");
#[cfg(test)]
const LANDSCAPE_MODEL_SHA256: &str =
    "93a779c5e5d835ad465eda76f48073b5abfb3da048a497efa29ffcb58498533e";
#[cfg(test)]
const GENERAL_MODEL_SHA256: &str =
    "1511ab5564cb0ae179cba9bcad6ad931003ccc71656f11fd270967023678c6a5";
const MODEL_CHANNELS: usize = 3;
const MAX_EDGE: u32 = 8192;
const MAX_PIXELS: u64 = 16_777_216;

/// The embedded network a frame selects by its orientation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Landscape,
    General,
}

impl Variant {
    fn for_frame(width: u32, height: u32) -> Self {
        if width > height {
            Self::Landscape
        } else {
            Self::General
        }
    }

    fn canvas(self) -> (usize, usize) {
        match self {
            Self::Landscape => (256, 144),
            Self::General => (256, 256),
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Landscape => LANDSCAPE_MODEL_BYTES,
            Self::General => GENERAL_MODEL_BYTES,
        }
    }
}

/// Where a frame lands inside a model's letterboxed canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Letterbox {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

/// Loads the network a frame needs and reuses it across frames and tracks.
pub(super) struct CutoutProcessor {
    loaded: Option<LoadedModel>,
}

impl CutoutProcessor {
    /// Create an empty processor. The network loads on the first frame, once
    /// the frame's orientation selects a variant.
    pub(super) fn new() -> Result<Self, String> {
        Ok(Self { loaded: None })
    }

    fn loaded_for(&mut self, variant: Variant) -> Result<&LoadedModel, String> {
        let reload = match self.loaded.as_ref() {
            Some(loaded) => loaded.variant != variant,
            None => true,
        };
        if reload {
            self.loaded = Some(LoadedModel::load(variant)?);
        }
        Ok(self
            .loaded
            .as_ref()
            .expect("a model was loaded for the frame"))
    }

    /// Return a full-resolution person mask, one `0..=1` value per input pixel
    /// in row-major order, where `1` is person and `0` is background.
    ///
    /// `bgra` must be an opaque, tightly packed BGRA frame of `size`; the alpha
    /// byte is ignored. The call runs a real inference and is CPU-bound, so it
    /// belongs on a worker thread rather than GTK's main thread.
    pub(super) fn matte(&mut self, bgra: &[u8], size: (u32, u32)) -> Result<Vec<f32>, String> {
        let (width, height) = size;
        let expected = frame_length(width, height)?;
        if bgra.len() != expected {
            return Err(format!(
                "The camera frame is {} bytes, but a {}x{} BGRA frame needs {expected}.",
                bgra.len(),
                width,
                height
            ));
        }
        let variant = Variant::for_frame(width, height);
        let (canvas_width, canvas_height) = variant.canvas();
        let layout = letterbox(width as usize, height as usize, canvas_width, canvas_height);
        let input = prepare_input(bgra, width as usize, height as usize, variant, layout);
        let tensor = Tensor::from_shape(&[1, canvas_height, canvas_width, MODEL_CHANNELS], &input)
            .map_err(|error| format!("Could not assemble the camera cutout input: {error}"))?;
        let loaded = self.loaded_for(variant)?;
        let outputs = loaded
            .model
            .run(tvec!(tensor.into()))
            .map_err(|error| format!("Camera cutout inference failed: {error}"))?;
        let output = outputs
            .first()
            .ok_or_else(|| "Camera cutout inference returned no mask.".to_string())?;
        let view = output
            .to_plain_array_view::<f32>()
            .map_err(|error| format!("The camera cutout mask has an unexpected type: {error}"))?;
        if view.shape() != [1, canvas_height, canvas_width, 1] {
            return Err(format!(
                "The camera cutout model returned a {:?} mask, not a {canvas_width}x{canvas_height} one.",
                view.shape()
            ));
        }
        let mut mask = Vec::with_capacity(canvas_width * canvas_height);
        for (index, value) in view.iter().enumerate() {
            if !value.is_finite() {
                return Err(format!(
                    "The camera cutout model returned a non-finite value at pixel {index}."
                ));
            }
            mask.push(value.clamp(0.0, 1.0));
        }
        let mut alpha = Vec::with_capacity(layout.width * layout.height);
        for y in layout.top..layout.top + layout.height {
            let start = y * canvas_width + layout.left;
            alpha.extend_from_slice(&mask[start..start + layout.width]);
        }
        Ok(resize_alpha(
            &alpha,
            layout.width,
            layout.height,
            width as usize,
            height as usize,
        ))
    }
}

/// One loaded network, tagged with the orientation it serves.
struct LoadedModel {
    variant: Variant,
    model: Arc<TypedRunnableModel>,
}

impl LoadedModel {
    fn load(variant: Variant) -> Result<Self, String> {
        Self::from_model_bytes(variant, variant.bytes())
    }

    fn from_model_bytes(variant: Variant, bytes: &[u8]) -> Result<Self, String> {
        let (canvas_width, canvas_height) = variant.canvas();
        let mut reader = std::io::Cursor::new(bytes);
        let model = tract_onnx::onnx()
            .model_for_read(&mut reader)
            .map_err(|error| format!("Could not read the camera cutout model: {error}"))?;
        if model.inputs.len() != 1 || model.outputs.len() != 1 {
            return Err(format!(
                "The camera cutout model must have one input and one output, but it has {} and {}.",
                model.inputs.len(),
                model.outputs.len()
            ));
        }
        let model = model
            .into_optimized()
            .map_err(|error| format!("Could not prepare the camera cutout model: {error}"))?;
        let input = model
            .input_fact(0)
            .map_err(|error| format!("Could not inspect the camera cutout model input: {error}"))?;
        let expected = [1, canvas_height, canvas_width, MODEL_CHANNELS];
        if input.datum_type != f32::datum_type()
            || input.shape.as_concrete() != Some(expected.as_slice())
        {
            return Err(format!(
                "The camera cutout model expects a {:?} input, not {:?}.",
                expected, input
            ));
        }
        let output = model.output_fact(0).map_err(|error| {
            format!("Could not inspect the camera cutout model output: {error}")
        })?;
        let expected_output = [1, canvas_height, canvas_width, 1];
        if output.datum_type != f32::datum_type() {
            return Err(format!(
                "The camera cutout model returns {:?}, not a float mask.",
                output.datum_type
            ));
        }
        if let Some(shape) = output
            .shape
            .as_concrete()
            .filter(|shape| *shape != expected_output.as_slice())
        {
            return Err(format!(
                "The camera cutout model returns a {shape:?} mask, not a {:?} one.",
                expected_output
            ));
        }
        let model = model
            .into_runnable()
            .map_err(|error| format!("Could not compile the camera cutout model: {error}"))?;
        Ok(Self { variant, model })
    }
}

/// The byte length a tightly packed BGRA frame needs.
fn frame_length(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 {
        return Err(format!("The camera frame is empty ({width}x{height})."));
    }
    if width > MAX_EDGE || height > MAX_EDGE {
        return Err(format!(
            "The camera frame is {width}x{height}, larger than the {MAX_EDGE}-pixel edge limit."
        ));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(format!(
            "The camera frame is {width}x{height}, larger than the {MAX_PIXELS}-pixel limit."
        ));
    }
    let bytes = pixels
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| format!("The camera frame is too large to address ({width}x{height})."))?;
    Ok(bytes)
}

/// The content rectangle a frame occupies inside a letterboxed canvas.
fn letterbox(width: usize, height: usize, canvas_width: usize, canvas_height: usize) -> Letterbox {
    let scale = (canvas_width as f64 / width as f64).min(canvas_height as f64 / height as f64);
    let content_width = ((width as f64 * scale).round() as usize).clamp(1, canvas_width);
    let content_height = ((height as f64 * scale).round() as usize).clamp(1, canvas_height);
    Letterbox {
        left: (canvas_width - content_width) / 2,
        top: (canvas_height - content_height) / 2,
        width: content_width,
        height: content_height,
    }
}

/// Convert a BGRA frame to the RGB, row-major `[0, 1]` tensor the network
/// expects, bilinearly resizing into the letterboxed content area.
fn prepare_input(
    bgra: &[u8],
    width: usize,
    height: usize,
    variant: Variant,
    layout: Letterbox,
) -> Vec<f32> {
    let (canvas_width, canvas_height) = variant.canvas();
    let mut input = vec![0.0; canvas_width * canvas_height * MODEL_CHANNELS];
    if layout.width == canvas_width
        && layout.height == canvas_height
        && width == canvas_width
        && height == canvas_height
    {
        let (targets, remainder) = input.as_chunks_mut::<MODEL_CHANNELS>();
        debug_assert!(remainder.is_empty());
        for (pixel, target) in bgra.as_chunks::<4>().0.iter().zip(targets) {
            target[0] = f32::from(pixel[2]) / 255.0;
            target[1] = f32::from(pixel[1]) / 255.0;
            target[2] = f32::from(pixel[0]) / 255.0;
        }
        return input;
    }
    let x_positions: Vec<_> = (0..layout.width)
        .map(|x| sample_positions(x, width as f32 / layout.width as f32, width))
        .collect();
    for content_y in 0..layout.height {
        let (top, bottom, vertical) =
            sample_positions(content_y, height as f32 / layout.height as f32, height);
        let row = (layout.top + content_y) * canvas_width + layout.left;
        for (content_x, &(left, right, horizontal)) in x_positions.iter().enumerate() {
            let target = (row + content_x) * MODEL_CHANNELS;
            for (channel, source) in [(0, 2), (1, 1), (2, 0)] {
                let top_left = f32::from(bgra[(top * width + left) * 4 + source]);
                let top_right = f32::from(bgra[(top * width + right) * 4 + source]);
                let bottom_left = f32::from(bgra[(bottom * width + left) * 4 + source]);
                let bottom_right = f32::from(bgra[(bottom * width + right) * 4 + source]);
                let top_value = top_left + (top_right - top_left) * horizontal;
                let bottom_value = bottom_left + (bottom_right - bottom_left) * horizontal;
                input[target + channel] =
                    (top_value + (bottom_value - top_value) * vertical) / 255.0;
            }
        }
    }
    input
}

/// Resize a single-channel mask with bilinear sampling and edge clamping.
fn resize_alpha(
    alpha: &[f32],
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
) -> Vec<f32> {
    debug_assert_eq!(alpha.len(), source_width * source_height);
    if width == source_width && height == source_height {
        return alpha.to_vec();
    }
    let x_positions: Vec<_> = (0..width)
        .map(|x| sample_positions(x, source_width as f32 / width as f32, source_width))
        .collect();
    let mut resized = Vec::with_capacity(width * height);
    for y in 0..height {
        let (top, bottom, vertical) =
            sample_positions(y, source_height as f32 / height as f32, source_height);
        for &(left, right, horizontal) in &x_positions {
            let top_left = alpha[top * source_width + left];
            let top_right = alpha[top * source_width + right];
            let bottom_left = alpha[bottom * source_width + left];
            let bottom_right = alpha[bottom * source_width + right];
            let top_value = top_left + (top_right - top_left) * horizontal;
            let bottom_value = bottom_left + (bottom_right - bottom_left) * horizontal;
            resized.push((top_value + (bottom_value - top_value) * vertical).clamp(0.0, 1.0));
        }
    }
    resized
}

/// The two source samples and the blend factor for one destination index.
#[inline]
fn sample_positions(index: usize, scale: f32, extent: usize) -> (usize, usize, f32) {
    let position = ((index as f32 + 0.5) * scale - 0.5).clamp(0.0, (extent - 1) as f32);
    let lower = position.floor() as usize;
    (lower, (lower + 1).min(extent - 1), position - lower as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn bundled_models_match_their_documented_checksums() {
        assert_eq!(LANDSCAPE_MODEL_BYTES.len(), 446_289);
        assert_eq!(
            format!("{:x}", Sha256::digest(LANDSCAPE_MODEL_BYTES)),
            LANDSCAPE_MODEL_SHA256
        );
        assert_eq!(GENERAL_MODEL_BYTES.len(), 447_596);
        assert_eq!(
            format!("{:x}", Sha256::digest(GENERAL_MODEL_BYTES)),
            GENERAL_MODEL_SHA256
        );
    }

    #[test]
    fn incompatible_model_bytes_are_rejected() {
        for variant in [Variant::Landscape, Variant::General] {
            let error = LoadedModel::from_model_bytes(variant, b"this is not an ONNX graph")
                .err()
                .expect("junk must not load");
            assert!(error.contains("camera cutout model"), "{error}");
        }
    }

    #[test]
    fn malformed_frames_are_rejected() {
        let mut processor = CutoutProcessor::new().expect("the processor starts empty");
        assert!(processor.matte(&[], (0, 0)).is_err());
        assert!(processor.matte(&[0; 4], (0, 1)).is_err());
        assert!(processor.matte(&[0; 4], (1, 0)).is_err());
        assert!(processor.matte(&[0; 3], (1, 1)).is_err());
        assert!(processor.matte(&[0; 8], (1, 1)).is_err());
        assert!(processor.matte(&[0; 4], (MAX_EDGE + 1, 1)).is_err());
        assert!(processor.matte(&[0; 4], (u32::MAX, u32::MAX)).is_err());
    }

    #[test]
    fn frames_select_the_network_by_orientation() {
        assert!(matches!(Variant::for_frame(1920, 1080), Variant::Landscape));
        assert!(matches!(Variant::for_frame(4, 3), Variant::Landscape));
        assert!(matches!(Variant::for_frame(512, 512), Variant::General));
        assert!(matches!(Variant::for_frame(1080, 1920), Variant::General));
        assert_eq!(Variant::Landscape.canvas(), (256, 144));
        assert_eq!(Variant::General.canvas(), (256, 256));
    }

    #[test]
    fn bgra_is_converted_to_normalised_rgb() {
        let red = [0, 0, 255, 255];
        let input = prepare_input(&red, 1, 1, Variant::General, letterbox(1, 1, 256, 256));
        assert_eq!(input.len(), 256 * 256 * MODEL_CHANNELS);
        assert_eq!(&input[0..3], &[1.0, 0.0, 0.0]);
        assert_eq!(&input[input.len() - 3..], &[1.0, 0.0, 0.0]);
        let blue = [255, 0, 0, 255];
        let input = prepare_input(&blue, 1, 1, Variant::General, letterbox(1, 1, 256, 256));
        assert_eq!(&input[0..3], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn square_input_fills_the_canvas() {
        let frame = [
            0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
        ];
        let input = prepare_input(&frame, 2, 2, Variant::General, letterbox(2, 2, 256, 256));
        assert_eq!(input.len(), 256 * 256 * MODEL_CHANNELS);
        assert_eq!(&input[0..3], &[1.0, 0.0, 0.0]);
        assert_eq!(&input[255 * 3..255 * 3 + 3], &[0.0, 1.0, 0.0]);
        let last_row = 255 * 256 * MODEL_CHANNELS;
        assert_eq!(&input[last_row..last_row + 3], &[0.0, 0.0, 1.0]);
        assert_eq!(
            &input[last_row + 255 * MODEL_CHANNELS..last_row + 256 * MODEL_CHANNELS],
            &[1.0, 1.0, 1.0]
        );
    }

    #[test]
    fn letterbox_keeps_the_subject_aspect() {
        let frame = [
            0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255, 0, 255, 255, 255,
            255, 0, 255, 255, 255, 255, 0, 255, 128, 128, 128, 255,
        ];
        let layout = letterbox(4, 2, 256, 144);
        assert_eq!(
            layout,
            Letterbox {
                left: 0,
                top: 8,
                width: 256,
                height: 128,
            }
        );
        let input = prepare_input(&frame, 4, 2, Variant::Landscape, layout);
        let row = |y: usize| &input[y * 256 * MODEL_CHANNELS..(y + 1) * 256 * MODEL_CHANNELS];
        assert!(row(0).iter().all(|&value| value == 0.0));
        assert!(row(7).iter().all(|&value| value == 0.0));
        assert_eq!(&row(8)[0..3], &[1.0, 0.0, 0.0]);
        assert_eq!(&row(8)[255 * 3..256 * 3], &[1.0, 1.0, 1.0]);
        assert_eq!(&row(135)[0..3], &[1.0, 1.0, 0.0]);
        assert_eq!(&row(135)[255 * 3..256 * 3], &[128.0 / 255.0; 3]);

        let portrait = [
            0, 0, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 128,
            128, 128, 255, 0, 255, 255, 255, 255, 0, 255, 255,
        ];
        let layout = letterbox(2, 4, 256, 256);
        assert_eq!(
            layout,
            Letterbox {
                left: 64,
                top: 0,
                width: 128,
                height: 256,
            }
        );
        let input = prepare_input(&portrait, 2, 4, Variant::General, layout);
        let at = |x: usize, y: usize| &input[(y * 256 + x) * MODEL_CHANNELS..][..3];
        assert_eq!(at(63, 0), &[0.0, 0.0, 0.0]);
        assert_eq!(at(64, 0), &[1.0, 0.0, 0.0]);
        assert_eq!(at(191, 0), &[0.0, 0.0, 1.0]);
        assert_eq!(at(192, 0), &[0.0, 0.0, 0.0]);
        assert_eq!(at(0, 255), &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn alpha_resize_is_bilinear_and_bounded() {
        let source = [0.0, 0.25, 0.75, 1.0];
        let resized = resize_alpha(&source, 2, 2, 4, 4);
        assert_eq!(resized.len(), 16);
        assert!((resized[0] - 0.0).abs() < 1e-6);
        assert!((resized[5] - 0.25).abs() < 1e-6);
        assert!((resized[10] - 0.75).abs() < 1e-6);
        assert!((resized[15] - 1.0).abs() < 1e-6);
        assert!(resized.iter().all(|value| (0.0..=1.0).contains(value)));
        assert_eq!(resize_alpha(&source, 2, 2, 2, 2), source);
    }

    #[test]
    fn real_inference_returns_a_valid_mask() {
        fn assert_send<T: Send>() {}
        assert_send::<CutoutProcessor>();

        let (width, height) = (96u32, 64u32);
        let mut frame = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                frame.push(((x * 7 + y * 13) & 0xff) as u8);
                frame.push(((x * 3 + y * 5) & 0xff) as u8);
                frame.push(((x ^ y) & 0xff) as u8);
                frame.push(255);
            }
        }
        let mut processor = CutoutProcessor::new().expect("the processor starts empty");
        let alpha = processor
            .matte(&frame, (width, height))
            .expect("a real frame produces a mask");
        assert_eq!(alpha.len(), (width * height) as usize);
        assert!(alpha.iter().all(|value| value.is_finite()));
        assert!(alpha.iter().all(|value| (0.0..=1.0).contains(value)));
        assert!(
            alpha.iter().any(|&value| value != alpha[0]),
            "the model should produce a per-pixel mask, not a constant"
        );
    }

    #[test]
    fn square_and_portrait_frames_return_a_valid_mask() {
        let mut processor = CutoutProcessor::new().expect("the processor starts empty");
        for (width, height) in [(64u32, 64u32), (64, 96)] {
            let mut frame = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    frame.push(((x * 7 + y * 13) & 0xff) as u8);
                    frame.push(((x * 3 + y * 5) & 0xff) as u8);
                    frame.push(((x ^ y) & 0xff) as u8);
                    frame.push(255);
                }
            }
            let alpha = processor
                .matte(&frame, (width, height))
                .expect("a real frame produces a mask");
            assert_eq!(alpha.len(), (width * height) as usize);
            assert!(alpha.iter().all(|value| value.is_finite()));
            assert!(alpha.iter().all(|value| (0.0..=1.0).contains(value)));
        }
    }
}
