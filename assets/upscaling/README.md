# Neural image enlargement

ApexShot bundles two QuickSRNetMedium ONNX models, one per mode. `model-2x.onnx`
enlarges by two and `model-4x.onnx` enlarges by four. Each mode runs its own
network at its native scale; the 2× option never runs the 4× model and
downsamples the result.

Both models are float32 and take an `image` input of shape `1×3×128×128`
with values in `[0, 1]`. `model-2x.onnx` returns `upscaled_image` of shape
`1×3×256×256`, and `model-4x.onnx` returns `1×3×512×512`.

## Runtime behavior

The models are embedded with `include_bytes!` and loaded once per process with
RTen 0.24's `Model::load_static_slice`, so enhancement needs no network access,
no separate inference installation, and no downloaded code. Captures are never
uploaded.

Images are processed in 128-pixel tiles with an 8-pixel halo on each side, and
each tile contributes only its central 112-pixel core to the output. The
network has seven 3×3 convolutions, so its receptive field reaches 7 pixels
from any output pixel; the 8-pixel halo covers that reach, so core pixels match
what the network would produce on the whole image. Halo pixels beyond the image
edge repeat the nearest edge pixel.

Alpha is handled outside the network. Each input sample is premultiplied by its
source alpha before inference, and each output colour is decoded by dividing by
the enlarged alpha. That enlarged alpha is the source alpha resampled with
Lanczos3, not something the network predicts. Output pixels whose alpha is zero
stay fully transparent, and an image with no transparency skips the alpha
resampling entirely.

Inputs must be non-empty and no larger than 1,048,576 pixels, with no edge over
4,096 pixels. Larger inputs are rejected before any model is loaded. The source
image is never modified.

The models can improve edges, but they cannot reliably reconstruct missing text
or guarantee new factual detail.

## Provenance

### `model-2x.onnx`

- Upstream checkpoint: `quicksrnet_medium_2x_checkpoint_float32.pth.tar` from
  <https://github.com/quic/aimet-model-zoo/releases/download/phase_2_january_artifacts/quicksrnet_medium_2x_checkpoint_float32.pth.tar>
- Checkpoint SHA-256: `a0d176b40a649e45a176c3b53f45e0237015f4f2c17b157ef5c81e38c4442a0d` (621,351 bytes)
- Bundled file SHA-256: `8f8ba6f4e46931d6744d15a5599a35a5f0ca9c1c9485028966b3203a8aecb35f`

The checkpoint is a PyTorch archive that `torch.load(..., weights_only=True)`
rejects, so it must never be loaded with `weights_only=False`. The conversion
instead reads the 14 weight and bias tensors from a weights-only state dict.
Each of those tensors is byte-identical to one of the checkpoint's storage
records.

The graph uses the same nodes, names, and wiring as the 4× export: seven
`Conv` layers, each followed by `Clip`, and a final `DepthToSpace`. Three
things differ from the 4× file. The final convolution emits 12 channels
(3 × 2²), `DepthToSpace` uses `mode=CRD` with `blocksize=2`, and the 14 weight
and bias initializers carry the 2× weights. The 4× file's intermediate
`value_info` entries recorded 48-channel shapes, so the 2× file omits them.

Checks run on the bundled file with ONNX 1.23.2 and ONNX Runtime 1.31.0:
`onnx.checker.check_model` passes, strict shape inference passes, and every
initializer equals its state-dict tensor. ONNX Runtime and direct PyTorch
agree on a seeded random 128×128 input with a maximum absolute difference of
2.15e-6. These are standalone checks, not ApexShot runtime tests.

### `model-4x.onnx`

- Upstream archive: `quicksrnetmedium-onnx-float.zip` from release `v0.64.0` of
  <https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/quicksrnetmedium/releases/v0.64.0/quicksrnetmedium-onnx-float.zip>
- Archive SHA-256: `89469f0c3d2ec51e1196d1c4ba4c52b31b6443c7022b1587c0537fe9754dbbab` (233,615 bytes)
- Model card: <https://huggingface.co/qualcomm/QuickSRNetMedium>
- Bundled file SHA-256: `4ed1fa606ab4a0f3d14ff60ef7bc9a9c42f4c918e0bef7e0b59e8069d7324ae2`

The archive contains `quicksrnetmedium.onnx` (34,270 bytes) with its weights
in the external `quicksrnetmedium.data` file (243,072 bytes), plus a
`metadata.json` that records QAIRT 2.50.0 and ONNX Runtime 1.27.1 as the export
tools. To reproduce the bundled file, extract the archive and run:

```python
import onnx

model = onnx.load("quicksrnetmedium-onnx-float/quicksrnetmedium.onnx", load_external_data=True)
onnx.checker.check_model(model)
onnx.save_model(model, "model-4x.onnx", save_as_external_data=False)
```

With ONNX 1.23.2 this reproduces the bundled file byte for byte.

## License

The upstream weights are distributed under the BSD 3-Clause license, which is
reproduced in `LICENSE` next to this file. Upstream license text:
<https://github.com/quic/aimet-model-zoo/blob/develop/LICENSE.md>. Binary
distributions that include these models must include `LICENSE`. The packaging
recipes install it, together with this README, at these paths:

| Package | License notice | This README |
| --- | --- | --- |
| Debian `.deb` | `/usr/share/doc/apexshot/upscaling.LICENSE` | `/usr/share/doc/apexshot/upscaling.md` |
| Flatpak | `/app/share/licenses/apexshot/upscaling.LICENSE` | `/app/share/licenses/apexshot/upscaling.md` |
| Fedora and openSUSE RPM | `%{_licensedir}/apexshot/upscaling.LICENSE` | `%{_docdir}/apexshot/upscaling.md` |
| Arch | `/usr/share/licenses/apexshot/upscaling.LICENSE` | `/usr/share/doc/apexshot/upscaling.md` |
| Nix | `$out/share/licenses/apexshot/upscaling.LICENSE` | `$out/share/doc/apexshot/upscaling.md` |
