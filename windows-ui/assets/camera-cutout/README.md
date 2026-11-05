# Camera cutout person segmentation

The Windows editor embeds a portrait segmentation network and the general
selfie segmentation network with `include_bytes!` and runs them on the CPU
through Tract's pure-Rust ONNX engine. Segmentation needs no network access,
Python, or a separately installed inference runtime.

## Selection and preprocessing

A frame wider than it is tall uses the portrait segmentation network. Its
192x192 input is resized to that shape, then its mask is resized back to the
camera's aspect ratio. Square and portrait frames use the general network,
letterboxed with zero bars. The mask is then upsampled with the original camera
image as a colour guide. This joint bilateral upsampling keeps
the soft mask while reducing leakage across contrasting image edges; when the
small guide has no matching colour, it falls back to bilinear interpolation.
It cannot correct a background region that the network confidently classifies
as a person.

For wide tracks, isolated, relatively small regions far from the main subject
are attenuated when their peak person confidence is low. Large regions, strong
separate subjects, and details close to the main subject are preserved.
Weak, detached hands or objects far from the subject can still be suppressed;
the cleanup is intentionally not applied to square or portrait tracks.

The model's documentation recommends a joint bilateral filter to improve
segmentation boundaries. Joint bilateral upsampling applies the same idea
while enlarging a low-resolution result:

- <https://github.com/google/mediapipe/blob/master/docs/solutions/selfie_segmentation.md>
- <https://johanneskopf.de/publications/jbu/paper/FinalPaper_0185.pdf>

Frames are processed independently, including during seeking. Temporal
smoothing without motion compensation can leave ghost trails and make a
seeked preview differ from export; a recurrent video matting model is a
possible future improvement but requires separate performance and quality
evaluation:

- <https://openaccess.thecvf.com/content/WACV2022/html/Lin_Robust_High-Resolution_Video_Matting_With_Temporal_Guidance_WACV_2022_paper.html>

## Portrait segmentation model

- **File:** `portrait_segmentation.onnx`, 6,196,913 bytes.
- **Task:** person/background segmentation for camera footage.
- **Input:** float32 `1x3x192x192`, NCHW RGB, normalized as `(byte / 128) - 1`.
- **Output:** float32 `1x192x192x2`, NHWC. The graph includes softmax;
  channel 1 is the person probability. Applying softmax again is incorrect.
- **SHA-256:** `6913acd125be55fd08e76072ed464146925223392954c437f3aa700e4da84b17`.

The model originates from the Apache-2.0 PP-HumanSeg project. The bundled ONNX
conversion was obtained from GitHub user `royshil`, commit
`8591913a41d07f54722e81844138b271e1184c93`, file
`data/models/pphumanseg_fp32.onnx`. It is included unchanged. The model's
published SPDX notice attributes PaddlePaddle Authors, Roy Shilkrot, and Kaito
Udagawa and specifies Apache-2.0. See `NOTICE` and `LICENSE`.

The older landscape model described below remains in the repository for
provenance but is not selected by the editor.

## General model

- **File:** `selfie_segmentation.onnx`, 447,596 bytes.
- **Task:** per-pixel person/background segmentation for a selfie camera.
- **Architecture:** the MediaPipe selfie segmentation network, general variant,
  256x256 input. The graph is a MobileNet-style encoder with depthwise
  convolutions, an average-pooling branch, transposed convolution and bilinear
  resizing.
- **Input:** `input_1:0`, float32, `1x256x256x3`, NHWC, RGB in `[0, 1]`.
- **Output:** `activation_10`, float32, `1x256x256x1`, NHWC, already passed
  through a sigmoid, so every value is a person probability in `[0, 1]`. Row
  order is top-to-bottom, matching the input.
- **Normalisation:** there is no mean/std subtraction or other scale node
  before the first convolution. The graph expects RGB bytes divided by 255;
  the editor feeds exactly that.

### Provenance

The upstream file is in the SelfieSegmentationBarracuda package, pinned at
commit `1851017d5797612014b8506d25a0215729ac8e51`:

<https://raw.githubusercontent.com/creativeIKEP/SelfieSegmentationBarracuda/1851017d5797612014b8506d25a0215729ac8e51/Packages/SelfieSegmentationBarracuda/ONNX/selfie_segmentation.onnx>

- Upstream file size: 447,596 bytes
- Upstream and bundled SHA-256:
  `1511ab5564cb0ae179cba9bcad6ad931003ccc71656f11fd270967023678c6a5`
- The repository declares Apache-2.0; its `LICENSE.md` is the same Apache
  License, Version 2.0 text as `LICENSE` beside this file.
- That repository's README says its ONNX files were downloaded from
  [PINTO_model_zoo](https://github.com/PINTO0309/PINTO_model_zoo) >
  [109_Selfie_Segmentation](https://github.com/PINTO0309/PINTO_model_zoo/tree/main/109_Selfie_Segmentation),
  which converts the TFLite selfie segmentation model published with the
  MediaPipe image segmentation solution.

The graph is bundled exactly as downloaded, at opset 11. Tract 0.23.9 loads it
without a rewrite and agrees with ONNX Runtime 1.31.1 to a maximum absolute
difference of `9.6e-6` on a 256x256 tensor prepared from a 512x512 portrait
frame.

## Landscape model

- **File:** `selfie_segmentation_landscape.onnx`, 446,289 bytes.
- **Task:** per-pixel person/background segmentation for a selfie camera.
- **Architecture:** the same MediaPipe selfie segmentation network, landscape
  variant, 256x144 input. The graph is a MobileNet-style encoder with depthwise
  convolutions, a global-average-pooling branch, transposed convolution and
  bilinear upsampling.
- **Input:** `input_1:0`, float32, `1x144x256x3`, NHWC, RGB in `[0, 1]`.
- **Output:** `segment_back`, float32, `1x144x256x1`, NHWC, already passed
  through a sigmoid, so every value is a person probability in `[0, 1]`.
  Row order is top-to-bottom, matching the input.
- **Normalisation:** like the general model, the graph expects RGB bytes
  divided by 255 with no mean/std subtraction.

### Provenance

The upstream conversion is the landscape ONNX file in the SelfieBarracuda
package, pinned at commit `47a67596979ed0081c9bb3ddaceea10aa875b800`:

<https://raw.githubusercontent.com/keijiro/SelfieBarracuda/47a67596979ed0081c9bb3ddaceea10aa875b800/Packages/jp.keijiro.mediapipe.selfie/ONNX/selfie_segmentation_landscape.onnx>

- Upstream file size: 446,301 bytes
- Upstream SHA-256: `ce062b4830ec7b2be2a328fcf5f7029b19d1b67b9662ac6ae5f40cb67d0d2bc1`
- Upstream declares Apache-2.0 in its package metadata; the licence text is
  next to this file in `LICENSE`.
- The conversion was made with `tflite2tensorflow` 1.8.5 from the TFLite
  selfie segmentation model published with the MediaPipe image segmentation
  solution, <https://developers.google.com/mediapipe/solutions/vision/image_segmenter>.

### Bundled-file conversion

The upstream file uses the deprecated opset-9 `Upsample` node three times.
Tract's typed translator does not accept that legacy form, so this copy
rewrites each one as the equivalent opset-10 `Resize` node and raises the graph
opset to 10. The weights, tensor names, node names, wiring and the three
`[1, 1, 2, 2]` scale tensors are unchanged. The transformation is:

```python
import onnx
from onnx import helper

model = onnx.load("selfie_segmentation_landscape.onnx")
nodes = []
for node in model.graph.node:
    if node.op_type == "Upsample":
        nodes.append(helper.make_node(
            "Resize", [node.input[0], node.input[1]], list(node.output),
            name=node.name, mode="linear",
        ))
    else:
        nodes.append(node)
del model.graph.node[:]
model.graph.node.extend(nodes)
for import_ in model.opset_import:
    if import_.domain == "":
        import_.version = 10
onnx.checker.check_model(model)
onnx.save(model, "selfie_segmentation_landscape.onnx")
```

`Resize`-10 always maps coordinates asymmetrically with the given scales,
which is the same mapping as the legacy linear `Upsample`. ONNX Runtime 1.31.1
produced bit-identical output for the upstream and rewritten files on a
512x512 portrait photograph and on a seeded random `144x256x3` tensor (maximum
absolute difference 0.0 in both cases). Tract 0.23.9 agrees with ONNX Runtime
on the rewritten file to within `7.5e-5` on the photograph and `1.8e-5` on the
random tensor.

- Bundled file size: 446,289 bytes
- Bundled SHA-256: `93a779c5e5d835ad465eda76f48073b5abfb3da048a497efa29ffcb58498533e`

## Licence and attribution

Both models are distributed under the Apache License, Version 2.0. The full
text is in `LICENSE` beside this file. Attribution:

- MediaPipe selfie segmentation models, Google LLC, Apache-2.0.
- General ONNX conversion and packaging: SelfieSegmentationBarracuda by IKEP,
  Apache-2.0, via PINTO_model_zoo's `109_Selfie_Segmentation`.
- Landscape ONNX conversion and packaging: SelfieBarracuda by Keijiro
  Takahashi, Apache-2.0.

The models are data, not executable code. Any binary distribution that includes
the embedded models must also carry the Apache-2.0 licence text and this
attribution.
