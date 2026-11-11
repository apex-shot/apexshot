//! GPU crop and scale for the export loop.
//!
//! The CPU path converts NV12 to RGBA, scales in software, and converts back.
//! When the export has no ripple or camera layer, the only full-frame
//! compositing left is the camera view: a crop of the source scaled to the
//! output. D3D11's video processor does that on the GPU and leaves the frame in
//! NV12, removing both colour conversions and the software scale. The cursor is
//! drawn afterwards on the NV12 frame, converting only the box it covers.
//!
//! The scaler is best-effort. A driver that cannot process NV12, a machine with
//! no D3D11 device, or `APEXSHOT_EXPORT_GPU=0` leaves the export on the CPU
//! path. A failed blit disables the scaler for the rest of the range.

use std::mem::ManuallyDrop;
use std::ptr;

use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC};

use super::super::pixels::{YuvMatrix, YuvSpace};
use super::super::zoom::ViewRect;

/// A D3D11 video processor that crops and scales one NV12 frame at a time.
pub(super) struct Scaler {
    context: ID3D11DeviceContext,
    video_context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    input_view: ID3D11VideoProcessorInputView,
    output_view: ID3D11VideoProcessorOutputView,
    upload: ID3D11Resource,
    input: ID3D11Resource,
    output: ID3D11Resource,
    readback: ID3D11Resource,
    _device: ID3D11Device,
    _video_device: ID3D11VideoDevice,
    _enumerator: ID3D11VideoProcessorEnumerator,
    source: (u32, u32),
    size: (u32, u32),
}

impl Scaler {
    /// Open the video processor for a source and output size.
    ///
    /// `fps` only feeds the content description's rate hints; the processor
    /// does not retime frames.
    pub(super) fn new(
        source: (u32, u32),
        size: (u32, u32),
        space: YuvSpace,
        fps: f64,
    ) -> Result<Self, String> {
        if std::env::var("APEXSHOT_EXPORT_GPU").is_ok_and(|value| value == "0") {
            return Err("disabled by APEXSHOT_EXPORT_GPU=0".to_string());
        }
        if source.0 < 2 || source.1 < 2 || size.0 < 2 || size.1 < 2 {
            return Err("the frame is too small for NV12".to_string());
        }
        if !source.0.is_multiple_of(2)
            || !source.1.is_multiple_of(2)
            || !size.0.is_multiple_of(2)
            || !size.1.is_multiple_of(2)
        {
            return Err("NV12 needs even frame dimensions".to_string());
        }
        if space.matrix == YuvMatrix::Bt2020 {
            return Err("BT.2020 is not handled by the video processor path".to_string());
        }

        let machine = super::machine::current();
        let Some(chosen) = machine.gpu.as_ref() else {
            return Err("no hardware GPU".to_string());
        };
        if !super::machine::gpu_stage_fits(machine, source, size) {
            return Err(format!("the GPU stage does not fit {}", chosen.name));
        }
        let adapter = super::machine::adapter(chosen);

        unsafe {
            let flags = D3D11_CREATE_DEVICE_FLAG(
                D3D11_CREATE_DEVICE_BGRA_SUPPORT.0 | D3D11_CREATE_DEVICE_VIDEO_SUPPORT.0,
            );
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                adapter.as_ref(),
                if adapter.is_some() {
                    D3D_DRIVER_TYPE_UNKNOWN
                } else {
                    D3D_DRIVER_TYPE_HARDWARE
                },
                HMODULE(ptr::null_mut()),
                flags,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|error| format!("could not open D3D11: {error}"))?;
            let device = device.ok_or_else(|| "D3D11 returned no device".to_string())?;
            let context = context.ok_or_else(|| "D3D11 returned no context".to_string())?;
            let video_device: ID3D11VideoDevice = device
                .cast()
                .map_err(|error| format!("the device has no video interface: {error}"))?;
            let video_context: ID3D11VideoContext = context
                .cast()
                .map_err(|error| format!("the context has no video interface: {error}"))?;

            let fps = DXGI_RATIONAL {
                Numerator: (fps.max(1.0) * 1000.0).round() as u32,
                Denominator: 1000,
            };
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: fps,
                InputWidth: source.0,
                InputHeight: source.1,
                OutputFrameRate: fps,
                OutputWidth: size.0,
                OutputHeight: size.1,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let enumerator = video_device
                .CreateVideoProcessorEnumerator(&content)
                .map_err(|error| format!("could not enumerate a video processor: {error}"))?;
            let support = enumerator
                .CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
                .map_err(|error| format!("the driver could not check NV12 support: {error}"))?;
            let input_support = D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32;
            let output_support = D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32;
            if support & input_support == 0 || support & output_support == 0 {
                return Err("the video processor does not take NV12 on both sides".to_string());
            }
            let processor = video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|error| format!("could not create a video processor: {error}"))?;

            // Bit 2 is the YCbCr matrix (0 = BT.601, 1 = BT.709) and bits 4-5
            // are the nominal range (1 = 16-235, 2 = 0-255).
            let matrix = match space.matrix {
                YuvMatrix::Bt709 => 1u32,
                _ => 0u32,
            };
            let range = if space.full_range { 2u32 } else { 1u32 };
            let color_space = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
                _bitfield: (matrix << 2) | (range << 4),
            };
            video_context.VideoProcessorSetStreamColorSpace(&processor, 0, &color_space);
            video_context.VideoProcessorSetOutputColorSpace(&processor, &color_space);

            let upload = create_texture(
                &device,
                source,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_WRITE.0 as u32,
            )?;
            let input = create_texture(
                &device,
                source,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_RENDER_TARGET.0 as u32,
                0,
            )?;
            let output = create_texture(
                &device,
                size,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_RENDER_TARGET.0 as u32,
                0,
            )?;
            let readback = create_texture(
                &device,
                size,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_READ.0 as u32,
            )?;

            let input_view = create_input_view(&video_device, &input, &enumerator)?;
            let output_view = create_output_view(&video_device, &output, &enumerator)?;
            let dest = RECT {
                left: 0,
                top: 0,
                right: size.0 as i32,
                bottom: size.1 as i32,
            };
            video_context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&dest));

            let upload = upload.cast().map_err(|error| error.to_string())?;
            let input = input.cast().map_err(|error| error.to_string())?;
            let output = output.cast().map_err(|error| error.to_string())?;
            let readback = readback.cast().map_err(|error| error.to_string())?;

            Ok(Self {
                context,
                video_context,
                processor,
                input_view,
                output_view,
                upload,
                input,
                output,
                readback,
                _device: device,
                _video_device: video_device,
                _enumerator: enumerator,
                source,
                size,
            })
        }
    }

    /// Crop `view` out of `input` and scale it into `output`, both NV12.
    pub(super) fn scale(
        &mut self,
        input: &[u8],
        view: ViewRect,
        output: &mut [u8],
    ) -> Result<(), String> {
        let source_bytes = self.source.0 as usize * self.source.1 as usize * 3 / 2;
        let output_bytes = self.size.0 as usize * self.size.1 as usize * 3 / 2;
        if input.len() != source_bytes || output.len() != output_bytes {
            return Err("the frame buffer has the wrong size".to_string());
        }

        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.upload, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped))
                .map_err(|error| format!("could not upload the frame: {error}"))?;
            let pitch = mapped.RowPitch as usize;
            let destination = mapped.pData as *mut u8;
            let width = self.source.0 as usize;
            let height = self.source.1 as usize;
            for row in 0..height {
                ptr::copy_nonoverlapping(
                    input.as_ptr().add(row * width),
                    destination.add(row * pitch),
                    width,
                );
            }
            let chroma = input.as_ptr().add(width * height);
            for row in 0..height / 2 {
                ptr::copy_nonoverlapping(
                    chroma.add(row * width),
                    destination.add(pitch * (height + row)),
                    width,
                );
            }
            self.context.Unmap(&self.upload, 0);
            self.context.CopyResource(&self.input, &self.upload);

            let rect = source_rect(view, self.source);
            self.video_context.VideoProcessorSetStreamSourceRect(
                &self.processor,
                0,
                true,
                Some(&rect),
            );

            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL(1),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: ptr::null_mut(),
                pInputSurface: ManuallyDrop::new(Some(self.input_view.clone())),
                ppFutureSurfaces: ptr::null_mut(),
                ppPastSurfacesRight: ptr::null_mut(),
                pInputSurfaceRight: ManuallyDrop::new(None),
                ppFutureSurfacesRight: ptr::null_mut(),
            };
            let mut streams = [stream];
            let result = self.video_context.VideoProcessorBlt(
                &self.processor,
                &self.output_view,
                0,
                &streams,
            );
            ManuallyDrop::drop(&mut streams[0].pInputSurface);
            result.map_err(|error| format!("the video processor could not blit: {error}"))?;

            self.context.CopyResource(&self.readback, &self.output);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| format!("could not read the frame back: {error}"))?;
            let pitch = mapped.RowPitch as usize;
            let source = mapped.pData as *const u8;
            let width = self.size.0 as usize;
            let height = self.size.1 as usize;
            for row in 0..height {
                ptr::copy_nonoverlapping(
                    source.add(row * pitch),
                    output.as_mut_ptr().add(row * width),
                    width,
                );
            }
            let chroma = output.as_mut_ptr().add(width * height);
            for row in 0..height / 2 {
                ptr::copy_nonoverlapping(
                    source.add(pitch * (height + row)),
                    chroma.add(row * width),
                    width,
                );
            }
            self.context.Unmap(&self.readback, 0);
        }
        Ok(())
    }
}

/// The source rectangle the video processor should sample.
///
/// The view rect is fractional; the processor takes whole pixels. Rounding
/// outwards keeps the frame edge in the picture.
fn source_rect(view: ViewRect, source: (u32, u32)) -> RECT {
    let max_x = source.0.saturating_sub(1) as f64;
    let max_y = source.1.saturating_sub(1) as f64;
    let left = view.0.floor().clamp(0.0, max_x);
    let top = view.1.floor().clamp(0.0, max_y);
    let right = (view.0 + view.2).ceil().clamp(left + 1.0, source.0 as f64);
    let bottom = (view.1 + view.3).ceil().clamp(top + 1.0, source.1 as f64);
    RECT {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    }
}

unsafe fn create_texture(
    device: &ID3D11Device,
    size: (u32, u32),
    usage: D3D11_USAGE,
    bind: u32,
    cpu_access: u32,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu_access,
        MiscFlags: 0,
    };
    let mut texture = None;
    device
        .CreateTexture2D(&desc, None, Some(&mut texture))
        .map_err(|error| format!("could not create a frame texture: {error}"))?;
    texture.ok_or_else(|| "D3D11 returned no frame texture".to_string())
}

unsafe fn create_input_view(
    device: &ID3D11VideoDevice,
    texture: &ID3D11Texture2D,
    enumerator: &ID3D11VideoProcessorEnumerator,
) -> Result<ID3D11VideoProcessorInputView, String> {
    let resource: ID3D11Resource = texture.cast().map_err(|error| error.to_string())?;
    let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
        FourCC: 0,
        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_VPIV {
                MipSlice: 0,
                ArraySlice: 0,
            },
        },
    };
    let mut view = None;
    device
        .CreateVideoProcessorInputView(&resource, enumerator, &desc, Some(&mut view))
        .map_err(|error| format!("could not create the input view: {error}"))?;
    view.ok_or_else(|| "D3D11 returned no input view".to_string())
}

unsafe fn create_output_view(
    device: &ID3D11VideoDevice,
    texture: &ID3D11Texture2D,
    enumerator: &ID3D11VideoProcessorEnumerator,
) -> Result<ID3D11VideoProcessorOutputView, String> {
    let resource: ID3D11Resource = texture.cast().map_err(|error| error.to_string())?;
    let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
        },
    };
    let mut view = None;
    device
        .CreateVideoProcessorOutputView(&resource, enumerator, &desc, Some(&mut view))
        .map_err(|error| format!("could not create the output view: {error}"))?;
    view.ok_or_else(|| "D3D11 returned no output view".to_string())
}

#[cfg(test)]
mod tests {
    use super::super::crop_scale_into;
    use super::*;
    use crate::video_editor::pixels::Nv12Converter;

    #[test]
    fn the_source_rect_rounds_outwards_and_stays_in_the_frame() {
        assert_eq!(
            source_rect((10.4, 20.6, 100.0, 50.0), (640, 480)),
            RECT {
                left: 10,
                top: 20,
                right: 111,
                bottom: 71
            }
        );
        assert_eq!(
            source_rect((-20.0, -10.0, 100.0, 50.0), (640, 480)),
            RECT {
                left: 0,
                top: 0,
                right: 80,
                bottom: 40
            }
        );
        assert_eq!(
            source_rect((600.0, 440.0, 100.0, 100.0), (640, 480)),
            RECT {
                left: 600,
                top: 440,
                right: 640,
                bottom: 480
            }
        );
    }

    #[test]
    #[ignore = "needs a D3D11 video processor"]
    fn the_gpu_scaler_keeps_a_flat_frame_flat() {
        let source = (320, 240);
        let size = (160, 120);
        let mut frame = vec![100u8; source.0 as usize * source.1 as usize];
        frame.extend(std::iter::repeat_n(
            128u8,
            source.0 as usize * source.1 as usize / 2,
        ));
        for (label, space) in [
            ("BT.601 limited", YuvSpace::default()),
            (
                "BT.709 limited",
                YuvSpace {
                    matrix: YuvMatrix::Bt709,
                    full_range: false,
                },
            ),
            (
                "BT.709 full",
                YuvSpace {
                    matrix: YuvMatrix::Bt709,
                    full_range: true,
                },
            ),
        ] {
            let mut gpu = match Scaler::new(source, size, space, 30.0) {
                Ok(scaler) => scaler,
                Err(error) => {
                    eprintln!("{label}: skipping: {error}");
                    continue;
                }
            };
            let mut output = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
            gpu.scale(&frame, (40.0, 30.0, 160.0, 120.0), &mut output)
                .expect("the GPU scale should succeed");
            let luma = &output[..size.0 as usize * size.1 as usize];
            let lowest = *luma.iter().min().unwrap();
            let highest = *luma.iter().max().unwrap();
            eprintln!("{label}: flat frame luma {lowest}..{highest}");
            assert!(
                (95..=105).contains(&i32::from(lowest)) && (95..=105).contains(&i32::from(highest)),
                "{label}: the GPU changed a flat luma frame: {lowest}..{highest}"
            );
            let chroma = &output[size.0 as usize * size.1 as usize..];
            assert!(
                chroma.iter().all(|value| (120..=136).contains(value)),
                "{label}: the GPU changed a flat chroma frame"
            );
        }
    }

    #[test]
    #[ignore = "needs a D3D11 video processor"]
    fn the_gpu_crop_and_scale_stays_close_to_the_cpu_path() {
        let space = YuvSpace::default();
        let source = (320, 240);
        let size = (160, 120);
        let frame = pattern(source);
        let mut gpu = match Scaler::new(source, size, space, 30.0) {
            Ok(scaler) => scaler,
            Err(error) => {
                eprintln!("skipping: {error}");
                return;
            }
        };
        let converter = Nv12Converter::new(space);
        let mut rgba = vec![0u8; source.0 as usize * source.1 as usize * 4];
        converter.to_rgba_into(&frame, source, &mut rgba);
        let mut scaled = vec![0u8; size.0 as usize * size.1 as usize * 4];
        let mut cpu_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let mut gpu_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        for (label, view) in [
            ("crop", (40.0, 30.0, 160.0, 120.0)),
            ("resize", (0.0, 0.0, 320.0, 240.0)),
        ] {
            gpu.scale(&frame, view, &mut gpu_frame)
                .expect("the GPU scale should succeed");
            crop_scale_into(&rgba, source, view, size, &mut scaled);
            converter.convert_into(&scaled, size, &mut cpu_frame);

            let luma = size.0 as usize * size.1 as usize;
            let luma_score = psnr(&cpu_frame[..luma], &gpu_frame[..luma]);
            let chroma_score = psnr(&cpu_frame[luma..], &gpu_frame[luma..]);
            let shift = best_shift(&cpu_frame[..luma], &gpu_frame[..luma], size);
            eprintln!(
                "{label}: GPU vs CPU PSNR luma {luma_score:.1} dB, chroma {chroma_score:.1} dB, best shift {shift:?}"
            );
            if label == "crop" {
                assert_eq!(
                    shift.0, 0,
                    "the GPU crop is shifted horizontally: {shift:?}"
                );
                assert_eq!(shift.1, 0, "the GPU crop is shifted vertically: {shift:?}");
            }
            assert!(
                luma_score > 30.0,
                "{label}: the GPU luma differs from the CPU path: {luma_score:.1} dB"
            );
            assert!(
                chroma_score > 25.0,
                "{label}: the GPU chroma differs from the CPU path: {chroma_score:.1} dB"
            );
        }
    }

    #[test]
    #[ignore = "needs two D3D11 video processors"]
    fn a_video_processor_matches_when_it_starts_mid_sequence() {
        let source = (320, 240);
        let size = (160, 120);
        let space = YuvSpace::default();
        let mut whole = match Scaler::new(source, size, space, 30.0) {
            Ok(scaler) => scaler,
            Err(error) => {
                eprintln!("skipping: {error}");
                return;
            }
        };
        let mut second =
            Scaler::new(source, size, space, 30.0).expect("the second scaler should open");
        let mut whole_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let mut second_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let luma = size.0 as usize * size.1 as usize;
        let mut lowest = f64::INFINITY;
        for index in 0..40 {
            let frame = pattern_with_offset(source, index);
            let view = (
                40.0 + f64::from(index) * 0.25,
                30.0 + f64::from(index) * 0.25,
                160.0 - f64::from(index) * 0.5,
                120.0 - f64::from(index) * 0.5,
            );
            whole.scale(&frame, view, &mut whole_frame).unwrap();
            if index >= 20 {
                second.scale(&frame, view, &mut second_frame).unwrap();
                lowest = lowest.min(psnr(&whole_frame[..luma], &second_frame[..luma]));
            }
        }
        eprintln!("fresh and warmed scalers: {lowest:.1} dB at worst");
        assert!(
            lowest > 50.0,
            "the video processor changed after restarting mid-sequence: {lowest:.1} dB"
        );
    }

    #[test]
    #[ignore = "needs two D3D11 video processors"]
    fn two_scalers_agree_on_a_zoomed_frame() {
        let source = (640, 360);
        let size = (640, 360);
        let frame = pattern(source);
        let view = (106.666_666_666_666_66, 60.0, 426.666_666_666_666_7, 240.0);
        let mut first = match Scaler::new(source, size, YuvSpace::default(), 30.0) {
            Ok(scaler) => scaler,
            Err(error) => {
                eprintln!("skipping: {error}");
                return;
            }
        };
        let mut second = Scaler::new(source, size, YuvSpace::default(), 30.0)
            .expect("the second scaler should open");
        let mut first_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let mut second_frame = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        first.scale(&frame, view, &mut first_frame).unwrap();
        second.scale(&frame, view, &mut second_frame).unwrap();
        let luma = size.0 as usize * size.1 as usize;
        eprintln!(
            "zoomed luma PSNR: {:.1} dB",
            psnr(&first_frame[..luma], &second_frame[..luma])
        );
        assert_eq!(first_frame, second_frame, "two scalers disagree on a zoom");
    }

    #[test]
    #[ignore = "needs two concurrent D3D11 video processors"]
    fn two_concurrent_scalers_agree() {
        let source = (640, 360);
        let size = (640, 360);
        let frame = pattern(source);
        let view = (106.666_666_666_666_66, 60.0, 426.666_666_666_666_7, 240.0);
        let mut outputs = Vec::new();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        let mut scaler = Scaler::new(source, size, YuvSpace::default(), 30.0)
                            .expect("the scaler should open");
                        let mut output = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
                        scaler.scale(&frame, view, &mut output).unwrap();
                        output
                    })
                })
                .collect();
            for handle in handles {
                outputs.push(handle.join().expect("the scaler thread should finish"));
            }
        });
        assert_eq!(outputs[0], outputs[1], "concurrent scalers disagree");
    }

    fn pattern_with_offset(size: (u32, u32), offset: u32) -> Vec<u8> {
        let mut frame = pattern(size);
        let width = size.0 as usize;
        let height = size.1 as usize;
        for y in 0..height {
            for x in 0..width {
                frame[y * width + x] = frame[y * width + x].wrapping_add((x as u32 + offset) as u8);
            }
        }
        frame
    }

    fn pattern(size: (u32, u32)) -> Vec<u8> {
        let width = size.0 as usize;
        let height = size.1 as usize;
        let mut frame = vec![0u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                frame[y * width + x] =
                    ((x * 255 / width.max(1) + y * 255 / height.max(1)) / 2) as u8;
            }
        }
        let chroma = width * height;
        for y in 0..height / 2 {
            for x in 0..width / 2 {
                let value = if (x / 20 + y / 20) % 2 == 0 { 96 } else { 160 };
                frame[chroma + y * width + x * 2] = value;
                frame[chroma + y * width + x * 2 + 1] = 255 - value;
            }
        }
        frame
    }

    fn psnr(left: &[u8], right: &[u8]) -> f64 {
        let error = left
            .iter()
            .zip(right)
            .map(|(left, right)| {
                let difference = f64::from(*left) - f64::from(*right);
                difference * difference
            })
            .sum::<f64>()
            / left.len().max(1) as f64;
        if error == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0 * 255.0 / error).log10()
        }
    }

    /// The integer shift that best aligns the two luma planes.
    fn best_shift(cpu: &[u8], gpu: &[u8], size: (u32, u32)) -> (i32, i32) {
        let width = size.0 as i32;
        let height = size.1 as i32;
        let mut best = ((0, 0), f64::INFINITY);
        for dy in -2..=2 {
            for dx in -2..=2 {
                let mut error = 0.0;
                let mut count = 0usize;
                for y in 2..height - 2 {
                    for x in 2..width - 2 {
                        let left = cpu[(y * width + x) as usize];
                        let right = gpu[((y + dy) * width + x + dx) as usize];
                        let difference = f64::from(left) - f64::from(right);
                        error += difference * difference;
                        count += 1;
                    }
                }
                let error = error / count.max(1) as f64;
                if error < best.1 {
                    best = ((dx, dy), error);
                }
            }
        }
        best.0
    }
}
