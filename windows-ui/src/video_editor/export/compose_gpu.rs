//! The GPU compositor stage.
//!
//! The CPU export converts NV12 to RGBA, composites in software, and converts
//! back. This stage does the whole thing in one D3D11 device: it uploads the
//! decoded NV12 frame, converts it to RGB into a mipmapped texture, composites
//! the background and the (trailed) card in a second pixel shader, and converts
//! the result back to NV12 with a luma pass and a chroma pass, then reads both
//! planes into one buffer.
//!
//! The stage is best-effort like the scaler: a machine with no D3D11 device, a
//! driver that cannot compile the shaders, or `APEXSHOT_EXPORT_GPU=0` leaves
//! the export on the CPU path.

use std::ptr;

use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_DRIVER_TYPE_HARDWARE,
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_SRV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8_UNORM,
    DXGI_FORMAT_R8_UNORM, DXGI_SAMPLE_DESC,
};

use super::super::pixels::{YuvMatrix, YuvSpace};

/// One frame's placement for the GPU stage, in output pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct StageFrame {
    /// Where the whole source lands this frame: x, y, width, height.
    pub card: [f32; 4],
    /// Where it landed at the start of the motion-blur trail; equal to `card`
    /// for no blur.
    pub before: [f32; 4],
    /// Corner radius as a fraction of the card's width (0 for square).
    pub radius_fraction: f32,
    /// Background scale about `pivot` (fractions of the output); 1 for none.
    pub background_zoom: f32,
    pub pivot: [f32; 2],
    /// The screen fills the frame: no card edges or background, the trail
    /// weighted as `ScreenTrail` does. False for a card on a canvas.
    pub screen: bool,
}

/// A D3D11 pipeline that composites one NV12 frame at a time.
pub(super) struct GpuStage {
    context: ID3D11DeviceContext,
    vertex: ID3D11VertexShader,
    decode: ID3D11PixelShader,
    compose: ID3D11PixelShader,
    luma_shader: ID3D11PixelShader,
    chroma_shader: ID3D11PixelShader,
    upload: ID3D11Texture2D,
    nv12: ID3D11Texture2D,
    _rgb: ID3D11Texture2D,
    _background: ID3D11Texture2D,
    _composite: ID3D11Texture2D,
    luma: ID3D11Texture2D,
    chroma: ID3D11Texture2D,
    luma_read: ID3D11Texture2D,
    chroma_read: ID3D11Texture2D,
    nv12_luma_srv: ID3D11ShaderResourceView,
    nv12_chroma_srv: ID3D11ShaderResourceView,
    rgb_rtv: ID3D11RenderTargetView,
    rgb_srv: ID3D11ShaderResourceView,
    background_srv: ID3D11ShaderResourceView,
    composite_rtv: ID3D11RenderTargetView,
    composite_srv: ID3D11ShaderResourceView,
    luma_rtv: ID3D11RenderTargetView,
    chroma_rtv: ID3D11RenderTargetView,
    sampler: ID3D11SamplerState,
    params: ID3D11Buffer,
    source: (u32, u32),
    output: (u32, u32),
    _levels: u32,
    full_range: f32,
    decode_coef: [f32; 4],
    luma_coef: [f32; 4],
    blue_coef: [f32; 4],
    red_coef: [f32; 4],
    _device: ID3D11Device,
}

impl GpuStage {
    /// `background`: opaque RGBA at `output` size, or None for black.
    pub(super) fn new(
        source: (u32, u32),
        output: (u32, u32),
        space: YuvSpace,
        background: Option<&[u8]>,
    ) -> Result<Self, String> {
        if std::env::var("APEXSHOT_EXPORT_GPU").is_ok_and(|value| value == "0") {
            return Err("disabled by APEXSHOT_EXPORT_GPU=0".to_string());
        }
        let even = |value: u32| value >= 2 && value.is_multiple_of(2);
        if !even(source.0) || !even(source.1) || !even(output.0) || !even(output.1) {
            return Err("NV12 needs even frame dimensions".to_string());
        }

        let machine = super::machine::current();
        let Some(chosen) = machine.gpu.as_ref() else {
            return Err("no hardware GPU".to_string());
        };
        if !super::machine::gpu_stage_fits(machine, source, output) {
            return Err(format!("the GPU stage does not fit {}", chosen.name));
        }
        let adapter = super::machine::adapter(chosen);

        unsafe {
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
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|error| format!("could not open D3D11: {error}"))?;
            let device = device.ok_or_else(|| "D3D11 returned no device".to_string())?;
            let context = context.ok_or_else(|| "D3D11 returned no context".to_string())?;

            let vertex = compile(SHADERS, "vs_main", "vs_5_0")?;
            let decode = compile(SHADERS, "ps_decode", "ps_5_0")?;
            let compose = compile(SHADERS, "ps_compose", "ps_5_0")?;
            let luma = compile(SHADERS, "ps_luma", "ps_5_0")?;
            let chroma = compile(SHADERS, "ps_chroma", "ps_5_0")?;

            let mut shader = None;
            device
                .CreateVertexShader(&blob_bytes(&vertex), None, Some(&mut shader))
                .map_err(|error| format!("could not create the vertex shader: {error}"))?;
            let vertex = shader.ok_or_else(|| "D3D11 returned no vertex shader".to_string())?;
            let decode = pixel_shader(&device, &decode, "decode")?;
            let compose = pixel_shader(&device, &compose, "compose")?;
            let luma = pixel_shader(&device, &luma, "luma")?;
            let chroma = pixel_shader(&device, &chroma, "chroma")?;

            let levels = levels(source);
            let upload = texture(
                &device,
                source,
                DXGI_FORMAT_NV12,
                1,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_WRITE.0 as u32,
                0,
            )?;
            let nv12 = texture(
                &device,
                source,
                DXGI_FORMAT_NV12,
                1,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_SHADER_RESOURCE.0 as u32,
                0,
                0,
            )?;
            let rgb = texture(
                &device,
                source,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                levels,
                D3D11_USAGE_DEFAULT,
                (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                0,
                D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            )?;
            let background = match background {
                Some(pixels) => {
                    let data = D3D11_SUBRESOURCE_DATA {
                        pSysMem: pixels.as_ptr() as _,
                        SysMemPitch: output.0 * 4,
                        SysMemSlicePitch: 0,
                    };
                    texture_with_data(&device, output, DXGI_FORMAT_R8G8B8A8_UNORM, &data)?
                }
                None => {
                    let black = [0u8, 0, 0, 255];
                    let data = D3D11_SUBRESOURCE_DATA {
                        pSysMem: black.as_ptr() as _,
                        SysMemPitch: 4,
                        SysMemSlicePitch: 0,
                    };
                    texture_with_data(&device, (1, 1), DXGI_FORMAT_R8G8B8A8_UNORM, &data)?
                }
            };
            let composite = texture(
                &device,
                output,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                1,
                D3D11_USAGE_DEFAULT,
                (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                0,
                0,
            )?;
            let luma_texture = texture(
                &device,
                output,
                DXGI_FORMAT_R8_UNORM,
                1,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_RENDER_TARGET.0 as u32,
                0,
                0,
            )?;
            let chroma_texture = texture(
                &device,
                (output.0 / 2, output.1 / 2),
                DXGI_FORMAT_R8G8_UNORM,
                1,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_RENDER_TARGET.0 as u32,
                0,
                0,
            )?;
            let luma_read = texture(
                &device,
                output,
                DXGI_FORMAT_R8_UNORM,
                1,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_READ.0 as u32,
                0,
            )?;
            let chroma_read = texture(
                &device,
                (output.0 / 2, output.1 / 2),
                DXGI_FORMAT_R8G8_UNORM,
                1,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_READ.0 as u32,
                0,
            )?;

            let nv12_luma_srv =
                shader_view(&device, &nv12, Some(&srv_desc(DXGI_FORMAT_R8_UNORM, 1)))?;
            let nv12_chroma_srv =
                shader_view(&device, &nv12, Some(&srv_desc(DXGI_FORMAT_R8G8_UNORM, 1)))?;
            let rgb_rtv = render_target(&device, &rgb)?;
            let rgb_srv = shader_view(
                &device,
                &rgb,
                Some(&srv_desc(DXGI_FORMAT_R8G8B8A8_UNORM, levels)),
            )?;
            let background_srv = shader_view(&device, &background, None)?;
            let composite_rtv = render_target(&device, &composite)?;
            let composite_srv = shader_view(&device, &composite, None)?;
            let luma_rtv = render_target(&device, &luma_texture)?;
            let chroma_rtv = render_target(&device, &chroma_texture)?;

            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: f32::MAX,
            };
            let mut sampler = None;
            device
                .CreateSamplerState(&sampler_desc, Some(&mut sampler))
                .map_err(|error| format!("could not create the sampler: {error}"))?;
            let sampler = sampler.ok_or_else(|| "D3D11 returned no sampler".to_string())?;

            let buffer_desc = D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<Params>() as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                MiscFlags: 0,
                StructureByteStride: 0,
            };
            let mut params = None;
            device
                .CreateBuffer(&buffer_desc, None, Some(&mut params))
                .map_err(|error| format!("could not create the parameter buffer: {error}"))?;
            let params = params.ok_or_else(|| "D3D11 returned no parameter buffer".to_string())?;

            let (decode_coef, luma_coef, blue_coef, red_coef) = colour_constants(space.matrix);

            Ok(Self {
                context,
                vertex,
                decode,
                compose,
                luma_shader: luma,
                chroma_shader: chroma,
                upload,
                nv12,
                _rgb: rgb,
                _background: background,
                _composite: composite,
                luma: luma_texture,
                chroma: chroma_texture,
                luma_read,
                chroma_read,
                nv12_luma_srv,
                nv12_chroma_srv,
                rgb_rtv,
                rgb_srv,
                background_srv,
                composite_rtv,
                composite_srv,
                luma_rtv,
                chroma_rtv,
                sampler,
                params,
                source,
                output,
                _levels: levels,
                full_range: if space.full_range { 1.0 } else { 0.0 },
                decode_coef,
                luma_coef,
                blue_coef,
                red_coef,
                _device: device,
            })
        }
    }

    /// Composite one frame: `input` is NV12 at the source size, `out` NV12 at
    /// the output size.
    pub(super) fn render(
        &mut self,
        input: &[u8],
        frame: StageFrame,
        out: &mut [u8],
    ) -> Result<(), String> {
        let (source_width, source_height) = self.source;
        let (width, height) = self.output;
        let source_bytes = source_width as usize * source_height as usize * 3 / 2;
        let output_bytes = width as usize * height as usize * 3 / 2;
        if input.len() != source_bytes || out.len() != output_bytes {
            return Err("the frame buffer has the wrong size".to_string());
        }

        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.upload, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped))
                .map_err(|error| format!("could not upload the frame: {error}"))?;
            let pitch = mapped.RowPitch as usize;
            let destination = mapped.pData as *mut u8;
            let source_w = source_width as usize;
            let source_h = source_height as usize;
            for row in 0..source_h {
                ptr::copy_nonoverlapping(
                    input.as_ptr().add(row * source_w),
                    destination.add(row * pitch),
                    source_w,
                );
            }
            let chroma = input.as_ptr().add(source_w * source_h);
            for row in 0..source_h / 2 {
                ptr::copy_nonoverlapping(
                    chroma.add(row * source_w),
                    destination.add(pitch * (source_h + row)),
                    source_w,
                );
            }
            self.context.Unmap(&self.upload, 0);
            self.context.CopyResource(&self.nv12, &self.upload);

            let params = Params {
                out_size: [width as f32, height as f32],
                source_size: [source_width as f32, source_height as f32],
                card_now: frame.card,
                card_before: frame.before,
                radius_fraction: frame.radius_fraction,
                background_zoom: frame.background_zoom.max(1.0),
                pivot: frame.pivot,
                quality: 16.0,
                span: 4.0,
                anisotropy: 4.0,
                full_range: self.full_range,
                decode_coef: self.decode_coef,
                luma_coef: self.luma_coef,
                blue_coef: self.blue_coef,
                red_coef: self.red_coef,
                mode: [if frame.screen { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(
                    &self.params,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .map_err(|error| format!("could not write the frame parameters: {error}"))?;
            ptr::copy_nonoverlapping(
                &params as *const Params as *const u8,
                mapped.pData as *mut u8,
                std::mem::size_of::<Params>(),
            );
            self.context.Unmap(&self.params, 0);

            self.context
                .IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.IASetInputLayout(None);
            self.context.VSSetShader(&self.vertex, None);
            self.context
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));

            self.pass(
                &self.rgb_rtv,
                self.source,
                &self.decode,
                &[
                    Some(self.nv12_luma_srv.clone()),
                    Some(self.nv12_chroma_srv.clone()),
                ],
            );
            self.context.GenerateMips(&self.rgb_srv);
            self.pass(
                &self.composite_rtv,
                self.output,
                &self.compose,
                &[
                    Some(self.rgb_srv.clone()),
                    Some(self.background_srv.clone()),
                ],
            );
            self.pass(
                &self.luma_rtv,
                self.output,
                &self.luma_shader,
                &[Some(self.composite_srv.clone())],
            );
            self.pass(
                &self.chroma_rtv,
                (width / 2, height / 2),
                &self.chroma_shader,
                &[Some(self.composite_srv.clone())],
            );

            self.context.CopyResource(&self.luma_read, &self.luma);
            self.context.CopyResource(&self.chroma_read, &self.chroma);

            let out_width = width as usize;
            let out_height = height as usize;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.luma_read, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| format!("could not read the luma plane back: {error}"))?;
            let pitch = mapped.RowPitch as usize;
            let plane = mapped.pData as *const u8;
            for row in 0..out_height {
                ptr::copy_nonoverlapping(
                    plane.add(row * pitch),
                    out.as_mut_ptr().add(row * out_width),
                    out_width,
                );
            }
            self.context.Unmap(&self.luma_read, 0);

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.chroma_read, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| format!("could not read the chroma plane back: {error}"))?;
            let pitch = mapped.RowPitch as usize;
            let plane = mapped.pData as *const u8;
            let luma_bytes = out_width * out_height;
            for row in 0..out_height / 2 {
                ptr::copy_nonoverlapping(
                    plane.add(row * pitch),
                    out.as_mut_ptr().add(luma_bytes + row * out_width),
                    out_width,
                );
            }
            self.context.Unmap(&self.chroma_read, 0);
        }
        Ok(())
    }

    /// One full-screen pass into `target`, then unbind so the next pass can
    /// read what it wrote.
    unsafe fn pass(
        &self,
        target: &ID3D11RenderTargetView,
        size: (u32, u32),
        shader: &ID3D11PixelShader,
        resources: &[Option<ID3D11ShaderResourceView>],
    ) {
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: size.0 as f32,
            Height: size.1 as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        self.context.RSSetViewports(Some(&[viewport]));
        self.context
            .OMSetRenderTargets(Some(&[Some(target.clone())]), None);
        self.context.PSSetShader(shader, None);
        self.context.PSSetShaderResources(0, Some(resources));
        self.context.Draw(3, 0);
        self.context.PSSetShaderResources(0, Some(&[None, None]));
        self.context.OMSetRenderTargets(None, None);
    }
}

/// The mip count the RGB texture carries: enough for the trail's LOD, capped.
fn levels(size: (u32, u32)) -> u32 {
    (1 + (size.0.max(size.1).max(1) as f32).log2().floor() as u32).min(8)
}

/// The colour constants the shaders use, scaled like `pixels.rs` does.
fn colour_constants(matrix: YuvMatrix) -> ([f32; 4], [f32; 4], [f32; 4], [f32; 4]) {
    let (decode, luma, blue, red): ([i32; 4], [i32; 3], [i32; 3], [i32; 3]) = match matrix {
        YuvMatrix::Bt601 => (
            [1436, -352, -731, 1815],
            [306, 601, 117],
            [-173, -339, 512],
            [512, -429, -83],
        ),
        YuvMatrix::Bt709 => (
            [1613, -192, -479, 1900],
            [218, 732, 74],
            [-117, -395, 512],
            [512, -465, -47],
        ),
        YuvMatrix::Bt2020 => (
            [1510, -168, -585, 1926],
            [269, 694, 61],
            [-143, -369, 512],
            [512, -471, -41],
        ),
    };
    let chroma = |values: [i32; 3]| {
        [
            values[0] as f32 / 1024.0,
            values[1] as f32 / 1024.0,
            values[2] as f32 / 1024.0,
            0.0,
        ]
    };
    (
        [
            decode[0] as f32 / 1024.0,
            decode[1] as f32 / 1024.0,
            decode[2] as f32 / 1024.0,
            decode[3] as f32 / 1024.0,
        ],
        chroma(luma),
        chroma(blue),
        chroma(red),
    )
}

/// Compile one entry point out of the shader source.
unsafe fn compile(source: &str, entry: &str, target: &str) -> Result<ID3DBlob, String> {
    let entry = format!("{entry}\0");
    let target = format!("{target}\0");
    let mut code = None;
    let mut errors = None;
    let result = D3DCompile(
        source.as_ptr() as _,
        source.len(),
        PCSTR::null(),
        None,
        None,
        PCSTR(entry.as_ptr()),
        PCSTR(target.as_ptr()),
        D3DCOMPILE_OPTIMIZATION_LEVEL3,
        0,
        &mut code,
        Some(&mut errors),
    );
    if let Err(error) = result {
        let text = errors
            .map(|blob| String::from_utf8_lossy(&blob_bytes(&blob)).into_owned())
            .unwrap_or_default();
        return Err(format!("could not compile the shaders: {error}: {text}"));
    }
    code.ok_or_else(|| "could not compile the shaders: no bytecode".to_string())
}

/// The bytes of a compiled shader or of a compiler error message.
unsafe fn blob_bytes(blob: &ID3DBlob) -> Vec<u8> {
    std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()).to_vec()
}

/// One of the pixel shaders, built from its compiled bytecode.
unsafe fn pixel_shader(
    device: &ID3D11Device,
    blob: &ID3DBlob,
    label: &str,
) -> Result<ID3D11PixelShader, String> {
    let mut shader = None;
    device
        .CreatePixelShader(&blob_bytes(blob), None, Some(&mut shader))
        .map_err(|error| format!("could not create the {label} shader: {error}"))?;
    shader.ok_or_else(|| format!("D3D11 returned no {label} shader"))
}

/// A 2D texture with the given description.
#[allow(clippy::too_many_arguments)]
unsafe fn texture(
    device: &ID3D11Device,
    size: (u32, u32),
    format: DXGI_FORMAT,
    mips: u32,
    usage: D3D11_USAGE,
    bind: u32,
    cpu_access: u32,
    misc: u32,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: mips,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu_access,
        MiscFlags: misc,
    };
    let mut texture = None;
    device
        .CreateTexture2D(&desc, None, Some(&mut texture))
        .map_err(|error| format!("could not create a stage texture: {error}"))?;
    texture.ok_or_else(|| "D3D11 returned no stage texture".to_string())
}

/// A 2D texture filled from `data` at creation time.
unsafe fn texture_with_data(
    device: &ID3D11Device,
    size: (u32, u32),
    format: DXGI_FORMAT,
    data: &D3D11_SUBRESOURCE_DATA,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    let data = data as *const D3D11_SUBRESOURCE_DATA;
    device
        .CreateTexture2D(&desc, Some(data), Some(&mut texture))
        .map_err(|error| format!("could not create the background texture: {error}"))?;
    texture.ok_or_else(|| "D3D11 returned no background texture".to_string())
}

/// A shader resource view over a whole texture, or with the given format.
fn srv_desc(format: DXGI_FORMAT, mips: u32) -> D3D11_SHADER_RESOURCE_VIEW_DESC {
    D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: mips,
            },
        },
    }
}

unsafe fn shader_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    desc: Option<&D3D11_SHADER_RESOURCE_VIEW_DESC>,
) -> Result<ID3D11ShaderResourceView, String> {
    let resource: ID3D11Resource = texture.cast().map_err(|error| error.to_string())?;
    let mut view = None;
    let desc = desc.map(|desc| desc as *const D3D11_SHADER_RESOURCE_VIEW_DESC);
    device
        .CreateShaderResourceView(&resource, desc, Some(&mut view))
        .map_err(|error| format!("could not create a shader view: {error}"))?;
    view.ok_or_else(|| "D3D11 returned no shader view".to_string())
}

unsafe fn render_target(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11RenderTargetView, String> {
    let resource: ID3D11Resource = texture.cast().map_err(|error| error.to_string())?;
    let mut view = None;
    device
        .CreateRenderTargetView(&resource, None, Some(&mut view))
        .map_err(|error| format!("could not create a render target: {error}"))?;
    view.ok_or_else(|| "D3D11 returned no render target".to_string())
}

const SHADERS: &str = r#"
cbuffer Params : register(b0)
{
    float2 outSize;
    float2 sourceSize;
    float4 cardNow;
    float4 cardBefore;
    float radiusFraction;
    float backgroundZoom;
    float2 pivot;
    float quality;
    float span;
    float anisotropy;
    float fullRange;
    float4 decodeCoef;   // red.v, green.u, green.v, blue.u  (inverse / 1024)
    float4 lumaCoef;     // kr, kg, kb, 0                    (/1024)
    float4 blueCoef;     // ur, ug, ub, 0
    float4 redCoef;      // vr, vg, vb, 0
    float4 mode;         // x: 1 = screen trail, 0 = card
};

Texture2D<float4> tex0 : register(t0);
Texture2D<float4> tex1 : register(t1);
SamplerState linearClamp : register(s0);

struct VsOut { float4 pos : SV_Position; };

VsOut vs_main(uint id : SV_VertexID)
{
    VsOut o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

// NV12 -> RGB, matching pixels.rs (bilinear chroma, the converter's matrix).
float4 ps_decode(VsOut i) : SV_Target
{
    float2 uv = i.pos.xy / sourceSize;
    float Y = tex0.SampleLevel(linearClamp, uv, 0).r;
    float2 C = tex1.SampleLevel(linearClamp, uv, 0).rg;
    float y = fullRange > 0.5 ? Y : (Y * 255.0 - 16.0) / 219.0;
    float2 c = fullRange > 0.5 ? (C * 255.0 - 128.0) / 255.0 : (C * 255.0 - 128.0) / 224.0;
    float r = y + decodeCoef.x * c.y;
    float g = y + decodeCoef.y * c.x + decodeCoef.z * c.y;
    float b = y + decodeCoef.w * c.x;
    return float4(saturate(float3(r, g, b)), 1.0);
}

float2 linear_interval(float a, float b, float lo, float hi)
{
    if (lo > hi) return float2(1, 0);
    float d = b - a;
    if (abs(d) < 1e-12) return (a >= lo && a <= hi) ? float2(0, 1) : float2(1, 0);
    float f = (lo - a) / d;
    float t = (hi - a) / d;
    return float2(max(min(f, t), 0.0), min(max(f, t), 1.0));
}

float2 intersect2(float2 a, float2 b) { return float2(max(a.x, b.x), min(a.y, b.y)); }
float span_length(float2 s) { return max(s.y - s.x, 0.0); }

float corner_coverage(float x, float y, float w, float h, float r)
{
    if (r <= 0.0) return 1.0;
    float dx = max(max(r - x, x - (w - r)), 0.0);
    float dy = max(max(r - y, y - (h - r)), 0.0);
    if (dx <= 0.0 || dy <= 0.0) return 1.0;
    return saturate(0.5 - (length(float2(dx, dy)) - r));
}

float bayer8(uint x, uint y)
{
    x &= 7; y &= 7;
    float l0 = (float)((((x >> 0) & 1) ^ ((y >> 0) & 1)) * 2 + ((y >> 0) & 1));
    float l1 = (float)((((x >> 1) & 1) ^ ((y >> 1) & 1)) * 2 + ((y >> 1) & 1));
    float l2 = (float)((((x >> 2) & 1) ^ ((y >> 2) & 1)) * 2 + ((y >> 2) & 1));
    return (l0 * 16.0 + l1 * 4.0 + l2) / 64.0;
}

// The screen filling the frame, trailed from cardBefore: a port of
// motion_blur.rs ScreenTrail::render (no edge coverage, opaque).
float3 screen_trail(float2 p)
{
    float4 B = cardBefore;
    float4 N = cardNow;
    float W = sourceSize.x;
    float H = sourceSize.y;
    float2 from = float2((p.x - B.x) / B.z * W - 0.5, (p.y - B.y) / B.w * H - 0.5);
    float2 to = float2((p.x - N.x) / N.z * W - 0.5, (p.y - N.y) / N.w * H - 0.5);
    float dx = (to.x - from.x) * B.z / W;
    float dy = (to.y - from.y) * B.w / H;
    float dist = length(float2(dx, dy));
    float count = dist < 1.0 ? 1.0 : clamp(ceil(dist / span), 1.0, quality);
    float phase = dist < 1.0 ? 0.5 : bayer8((uint)p.x, (uint)p.y);
    float2 stepTexel = (to - from) / count;
    float major = length(stepTexel);
    float minor = max(max(W / B.z, H / B.w), 1e-6);
    float taps = clamp(ceil(major / minor), 1.0, anisotropy);
    float lod = max(log2(max(major / taps, minor)), 0.0);
    float3 sum = 0;
    [loop]
    for (float s = 0; s < count; s += 1.0)
    {
        float t = (s + phase) / count;
        float2 c = from + (to - from) * t;
        float3 average = 0;
        [loop]
        for (float k = 0; k < taps; k += 1.0)
        {
            float o = (k + 0.5) / taps - 0.5;
            float2 st = clamp(c + stepTexel * o, float2(0, 0), float2(W - 1.0, H - 1.0));
            average += tex0.SampleLevel(linearClamp, (st + 0.5) / sourceSize, lod).rgb;
        }
        average /= taps;
        sum += average * average;
    }
    return sqrt(sum / count);
}

// The source placed at cardNow, trailed from cardBefore, over the background.
// A port of motion_blur.rs CardTrail::render (same sampling and weighting).
float4 ps_compose(VsOut i) : SV_Target
{
    float2 p = i.pos.xy;
    float3 background = 0;
    {
        float2 centre = pivot * outSize;
        float2 q = (p - centre) / backgroundZoom + centre;
        background = tex1.SampleLevel(linearClamp, q / outSize, 0).rgb;
    }
    if (mode.x > 0.5)
    {
        return float4(screen_trail(p), 1.0);
    }
    float4 B = cardBefore;
    float4 N = cardNow;
    float W = sourceSize.x;
    float H = sourceSize.y;
    float u0 = (p.x - B.x) / B.z;
    float u1 = (p.x - N.x) / N.z;
    float v0 = (p.y - B.y) / B.w;
    float v1 = (p.y - N.y) / N.w;
    float eu = 0.5 / N.z;
    float ev = 0.5 / N.w;
    float2 innerSpan = intersect2(linear_interval(u0, u1, eu, 1.0 - eu), linear_interval(v0, v1, ev, 1.0 - ev));
    float2 outerSpan = intersect2(linear_interval(u0, u1, -eu, 1.0 + eu), linear_interval(v0, v1, -ev, 1.0 + ev));
    float coverage = 0.5 * (span_length(innerSpan) + span_length(outerSpan));
    if (coverage <= 1e-4) return float4(background, 1.0);
    float ta = outerSpan.x;
    float tb = outerSpan.y;
    float2 startTexel = float2((u0 + (u1 - u0) * ta) * W - 0.5, (v0 + (v1 - v0) * ta) * H - 0.5);
    float2 endTexel = float2((u0 + (u1 - u0) * tb) * W - 0.5, (v0 + (v1 - v0) * tb) * H - 0.5);
    float dx = (endTexel.x - startTexel.x) * N.z / W;
    float dy = (endTexel.y - startTexel.y) * N.w / H;
    float dist = length(float2(dx, dy));
    float count = dist < 1.0 ? 1.0 : clamp(ceil(dist / span), 1.0, quality);
    float phase = dist < 1.0 ? 0.5 : bayer8((uint)p.x, (uint)p.y);
    float2 stepTexel = (endTexel - startTexel) / count;
    float major = length(stepTexel);
    float minor = max(max(W / N.z, H / N.w), 1e-6);
    float taps = clamp(ceil(major / minor), 1.0, anisotropy);
    float lod = max(log2(max(major / taps, minor)), 0.0);
    float3 sum = 0;
    float weight = 0;
    [loop]
    for (float s = 0; s < count; s += 1.0)
    {
        float t = ta + (tb - ta) * (s + phase) / count;
        float u = u0 + (u1 - u0) * t;
        float v = v0 + (v1 - v0) * t;
        float cw = B.z + (N.z - B.z) * t;
        float ch = B.w + (N.w - B.w) * t;
        float corner = corner_coverage(u * cw, v * ch, cw, ch, min(radiusFraction * cw, min(cw, ch) * 0.5));
        if (corner <= 0.0) continue;
        float2 c = float2(u * W - 0.5, v * H - 0.5);
        float3 average = 0;
        [loop]
        for (float k = 0; k < taps; k += 1.0)
        {
            float o = (k + 0.5) / taps - 0.5;
            float2 st = clamp(c + stepTexel * o, float2(0, 0), float2(W - 1.0, H - 1.0));
            average += tex0.SampleLevel(linearClamp, (st + 0.5) / sourceSize, lod).rgb;
        }
        average /= taps;
        sum += corner * average * average;
        weight += corner;
    }
    if (weight <= 0.0) return float4(background, 1.0);
    float alpha = saturate(coverage * weight / count);
    float3 colour = sqrt(sum / weight) * alpha;
    return float4(background * (1.0 - alpha) + colour, 1.0);
}

float luma_of(float3 rgb)
{
    float y = dot(rgb, lumaCoef.xyz);
    return fullRange > 0.5 ? y : (16.0 + 219.0 * y) / 255.0;
}

float ps_luma(VsOut i) : SV_Target
{
    return luma_of(tex0.Load(int3((int2)i.pos.xy, 0)).rgb);
}

float2 ps_chroma(VsOut i) : SV_Target
{
    int2 base = (int2)i.pos.xy * 2;
    float3 rgb = (tex0.Load(int3(base, 0)).rgb + tex0.Load(int3(base + int2(1, 0), 0)).rgb
        + tex0.Load(int3(base + int2(0, 1), 0)).rgb + tex0.Load(int3(base + int2(1, 1), 0)).rgb) * 0.25;
    float u = dot(rgb, blueCoef.xyz);
    float v = dot(rgb, redCoef.xyz);
    float2 c = float2(u, v);
    return fullRange > 0.5 ? saturate((128.0 + 255.0 * c) / 255.0) : saturate((128.0 + 224.0 * c) / 255.0);
}
"#;

/// The constant buffer, laid out to match `Params` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Params {
    out_size: [f32; 2],
    source_size: [f32; 2],
    card_now: [f32; 4],
    card_before: [f32; 4],
    radius_fraction: f32,
    background_zoom: f32,
    pivot: [f32; 2],
    quality: f32,
    span: f32,
    anisotropy: f32,
    full_range: f32,
    decode_coef: [f32; 4],
    luma_coef: [f32; 4],
    blue_coef: [f32; 4],
    red_coef: [f32; 4],
    /// x: 1 for the screen trail, 0 for a card; yzw unused.
    mode: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::super::super::motion_blur::ScreenTrail;
    use super::super::crop_scale_into;
    use super::*;
    use crate::video_editor::pixels::{Nv12Converter, YuvSpace};

    #[test]
    fn the_identity_stage_returns_the_frame() {
        let size = (320, 180);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let Ok(mut stage) = GpuStage::new(size, size, space, None) else {
            eprintln!("skipping: no D3D11 stage");
            return;
        };
        let converter = Nv12Converter::new(space);
        let rgba = pattern(size);
        let mut input = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        converter.convert_into(&rgba, size, &mut input);
        let mut output = vec![0u8; input.len()];
        let frame = StageFrame {
            card: [0.0, 0.0, 320.0, 180.0],
            before: [0.0, 0.0, 320.0, 180.0],
            radius_fraction: 0.0,
            background_zoom: 1.0,
            pivot: [0.5, 0.5],
            screen: true,
        };
        stage
            .render(&input, frame, &mut output)
            .expect("the GPU stage should composite");
        // The CPU path converts NV12 to RGB and back too, and on the hard-edged
        // checker that round trip is not lossless, so it is the reference.
        let mut decoded = vec![0u8; size.0 as usize * size.1 as usize * 4];
        converter.to_rgba_into(&input, size, &mut decoded);
        let mut expected = vec![0u8; input.len()];
        converter.convert_into(&decoded, size, &mut expected);
        let luma = size.0 as usize * size.1 as usize;
        let score = psnr(&expected[..luma], &output[..luma]);
        eprintln!("identity luma PSNR: {score:.1} dB");
        assert!(score >= 45.0, "the stage changed the frame: {score:.1} dB");
    }

    #[test]
    fn the_stage_crops_like_the_cpu_path() {
        let source = (640, 360);
        let size = (320, 180);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let Ok(mut stage) = GpuStage::new(source, size, space, None) else {
            eprintln!("skipping: no D3D11 stage");
            return;
        };
        let converter = Nv12Converter::new(space);
        let rgba = pattern(source);
        let mut input = vec![0u8; source.0 as usize * source.1 as usize * 3 / 2];
        converter.convert_into(&rgba, source, &mut input);
        let view = (100.0, 50.0, 320.0, 180.0);
        let card = [
            -view.0 * f64::from(size.0) / view.2,
            -view.1 * f64::from(size.1) / view.3,
            f64::from(source.0) * f64::from(size.0) / view.2,
            f64::from(source.1) * f64::from(size.1) / view.3,
        ];
        let card = card.map(|value| value as f32);
        let mut output = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let frame = StageFrame {
            card,
            before: card,
            radius_fraction: 0.0,
            background_zoom: 1.0,
            pivot: [0.5, 0.5],
            screen: true,
        };
        stage
            .render(&input, frame, &mut output)
            .expect("the GPU stage should composite");

        let mut cropped = vec![0u8; size.0 as usize * size.1 as usize * 4];
        crop_scale_into(&rgba, source, view, size, &mut cropped);
        let mut cpu = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        converter.convert_into(&cropped, size, &mut cpu);

        let luma = size.0 as usize * size.1 as usize;
        let score = psnr(&cpu[..luma], &output[..luma]);
        eprintln!("crop luma PSNR: {score:.1} dB");
        assert!(
            score >= 38.0,
            "the stage crops differently from the CPU path: {score:.1} dB"
        );
    }

    #[test]
    fn the_stage_blurs_like_the_cpu_trail() {
        let source = (640, 360);
        let size = (640, 360);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let Ok(mut stage) = GpuStage::new(source, size, space, None) else {
            eprintln!("skipping: no D3D11 stage");
            return;
        };
        let converter = Nv12Converter::new(space);
        let rgba = pattern(source);
        let mut input = vec![0u8; source.0 as usize * source.1 as usize * 3 / 2];
        converter.convert_into(&rgba, source, &mut input);
        let now = (80.0, 45.0, 480.0, 270.0);
        let previous = (60.0, 30.0, 520.0, 292.5);
        let rect = |view: (f64, f64, f64, f64)| {
            [
                -view.0 * f64::from(size.0) / view.2,
                -view.1 * f64::from(size.1) / view.3,
                f64::from(source.0) * f64::from(size.0) / view.2,
                f64::from(source.1) * f64::from(size.1) / view.3,
            ]
        };
        let card = rect(now).map(|value| value as f32);
        let old = rect(previous);
        let strength = 1.0;
        let before = std::array::from_fn(|i| {
            let card = f64::from(card[i]);
            (card + (old[i] - card) * strength) as f32
        });
        let mut output = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        let frame = StageFrame {
            card,
            before,
            radius_fraction: 0.0,
            background_zoom: 1.0,
            pivot: [0.5, 0.5],
            screen: true,
        };
        stage
            .render(&input, frame, &mut output)
            .expect("the GPU stage should composite");

        let mut trail = vec![0u8; size.0 as usize * size.1 as usize * 4];
        ScreenTrail::new(now, previous, (640.0, 360.0), (640, 360), 60.0, 1.0)
            .expect("the trail should have a span")
            .render(&rgba, (640, 360), (640, 360), &mut trail, 16, 4.0, 4);
        let mut cpu = vec![0u8; size.0 as usize * size.1 as usize * 3 / 2];
        converter.convert_into(&trail, size, &mut cpu);

        let luma = size.0 as usize * size.1 as usize;
        let score = psnr(&cpu[..luma], &output[..luma]);
        eprintln!("trail luma PSNR: {score:.1} dB");
        assert!(
            score >= 32.0,
            "the stage blurs differently from the CPU trail: {score:.1} dB"
        );
    }

    #[test]
    fn the_stage_draws_a_rounded_card_over_the_background() {
        let output_size = (400, 300);
        let source = (320, 240);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let background = solid(output_size, [0, 200, 0, 255]);
        let Ok(mut stage) = GpuStage::new(source, output_size, space, Some(&background)) else {
            eprintln!("skipping: no D3D11 stage");
            return;
        };
        let converter = Nv12Converter::new(space);
        let rgba = pattern(source);
        let mut input = vec![0u8; source.0 as usize * source.1 as usize * 3 / 2];
        converter.convert_into(&rgba, source, &mut input);
        let mut output = vec![0u8; output_size.0 as usize * output_size.1 as usize * 3 / 2];
        let card = [40.0, 30.0, 320.0, 240.0];
        let frame = StageFrame {
            card,
            before: card,
            radius_fraction: 0.1,
            background_zoom: 1.0,
            pivot: [0.5, 0.5],
            screen: false,
        };
        stage
            .render(&input, frame, &mut output)
            .expect("the GPU stage should composite");

        let mut drawn = vec![0u8; output_size.0 as usize * output_size.1 as usize * 4];
        converter.to_rgba_into(&output, output_size, &mut drawn);
        let pixel = |x: usize, y: usize| {
            let at = (y * output_size.0 as usize + x) * 4;
            [drawn[at], drawn[at + 1], drawn[at + 2]]
        };
        let green = |[r, g, _]: [u8; 3]| g > 150 && r < 80;
        assert!(
            green(pixel(5, 5)),
            "the background is not green: {:?}",
            pixel(5, 5)
        );
        assert!(
            green(pixel(41, 31)),
            "the rounded corner is not cut out: {:?}",
            pixel(41, 31)
        );
        assert!(
            !green(pixel(200, 150)),
            "the card is not drawn: {:?}",
            pixel(200, 150)
        );
        assert!(
            !green(pixel(200, 31)),
            "the card's top edge is missing: {:?}",
            pixel(200, 31)
        );
    }

    #[test]
    fn a_zoomed_background_moves_about_its_pivot() {
        let output_size = (400, 300);
        let source = (320, 240);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let background = halves(output_size);
        let Ok(mut stage) = GpuStage::new(source, output_size, space, Some(&background)) else {
            eprintln!("skipping: no D3D11 stage");
            return;
        };
        let converter = Nv12Converter::new(space);
        let rgba = pattern(source);
        let mut input = vec![0u8; source.0 as usize * source.1 as usize * 3 / 2];
        converter.convert_into(&rgba, source, &mut input);
        let mut output = vec![0u8; output_size.0 as usize * output_size.1 as usize * 3 / 2];
        let card = [1000.0, 1000.0, 10.0, 10.0];
        let frame = |zoom: f32, pivot: [f32; 2]| StageFrame {
            card,
            before: card,
            radius_fraction: 0.0,
            background_zoom: zoom,
            pivot,
            screen: false,
        };
        let at = 10 * output_size.0 as usize + 390;
        stage
            .render(&input, frame(1.0, [0.5, 0.5]), &mut output)
            .expect("the GPU stage should composite");
        assert!(
            output[at] > 200,
            "the unzoomed background is not white: {}",
            output[at]
        );
        stage
            .render(&input, frame(2.0, [0.0, 0.5]), &mut output)
            .expect("the GPU stage should composite");
        assert!(
            output[at] < 50,
            "the zoomed background did not move: {}",
            output[at]
        );
    }

    /// Opaque RGBA filled with one colour.
    fn solid(size: (u32, u32), colour: [u8; 4]) -> Vec<u8> {
        let mut frame = vec![0u8; size.0 as usize * size.1 as usize * 4];
        for pixel in frame.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&colour);
        }
        frame
    }

    /// Opaque RGBA, black on the left half and white on the right.
    fn halves(size: (u32, u32)) -> Vec<u8> {
        let width = size.0 as usize;
        let mut frame = vec![0u8; width * size.1 as usize * 4];
        for (index, pixel) in frame.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = if index % width < width / 2 { 0 } else { 255 };
            pixel.copy_from_slice(&[value, value, value, 255]);
        }
        frame
    }

    /// Smooth gradient plus a hard-edged checker, so both the sampling and the
    /// edges show up in a comparison.
    fn pattern(size: (u32, u32)) -> Vec<u8> {
        let width = size.0 as usize;
        let height = size.1 as usize;
        let mut frame = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) * 4;
                frame[at] = (x * 255 / width.max(1)) as u8;
                frame[at + 1] = (y * 255 / height.max(1)) as u8;
                frame[at + 2] = if (x / 16 + y / 16) % 2 == 0 { 200 } else { 40 };
                frame[at + 3] = 255;
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

    #[test]
    #[ignore = "benchmark: renders 120 frames on the GPU"]
    fn stage_throughput_benchmark() {
        let source = (1920, 1080);
        let space = YuvSpace::from_probe(Some("bt709"), None);
        let background = solid((2160, 1260), [40, 60, 120, 255]);
        for (label, output, background, screen) in [
            ("screen 1080p", (1920, 1080), None, true),
            (
                "card 2160x1260",
                (2160, 1260),
                Some(background.as_slice()),
                false,
            ),
        ] {
            let Ok(mut stage) = GpuStage::new(source, output, space, background) else {
                eprintln!("skipping: no D3D11 stage");
                return;
            };
            let converter = Nv12Converter::new(space);
            let mut input = vec![0u8; source.0 as usize * source.1 as usize * 3 / 2];
            converter.convert_into(&pattern(source), source, &mut input);
            let mut out = vec![0u8; output.0 as usize * output.1 as usize * 3 / 2];
            for (motion, blur) in [("still", 0.0f32), ("zoom transition", 1.0)] {
                let started = std::time::Instant::now();
                for frame in 0..60 {
                    // A 2x zoom moving in: the card grows by a few percent a frame.
                    let grow = 1.0 + frame as f32 / 60.0;
                    let card = [
                        -(grow - 1.0) * 600.0,
                        -(grow - 1.0) * 300.0,
                        output.0 as f32 * grow,
                        output.1 as f32 * grow,
                    ];
                    let previous = 1.0 + (frame as f32 - 1.0).max(0.0) / 60.0 - 0.04 * blur;
                    let before = [
                        -(previous - 1.0) * 600.0,
                        -(previous - 1.0) * 300.0,
                        output.0 as f32 * previous,
                        output.1 as f32 * previous,
                    ];
                    let frame = StageFrame {
                        card,
                        before: if blur > 0.0 { before } else { card },
                        radius_fraction: if screen { 0.0 } else { 0.02 },
                        background_zoom: 1.0 + (grow - 1.0) * 0.125,
                        pivot: [0.6, 0.4],
                        screen,
                    };
                    stage.render(&input, frame, &mut out).expect("render");
                }
                eprintln!(
                    "[gpu stage] {label}, {motion}: {:.1} ms/frame",
                    started.elapsed().as_secs_f64() * 1000.0 / 60.0
                );
            }
        }
    }
}
