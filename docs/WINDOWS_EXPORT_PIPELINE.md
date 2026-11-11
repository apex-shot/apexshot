# Windows export pipeline

`windows-ui/src/video_editor/export.rs` composites the composition and writes
an H.264 MP4. This note covers the parts that choose how the work is split and
which encoder writes it.

## Paths

The exporter picks the cheapest path that still produces the requested frame:

1. **NV12 passthrough.** When the export has no zoom, cursor, click ripple, or
   camera layer, and the output size equals the source size, the decoder's
   NV12 frames go to the encoder unchanged. The compositor's two full-frame
   colour conversions are skipped. This is the common case for a plain screen
   recording and is roughly twice as fast as the full path.
2. **Sequential composite.** Zoom, cursor overlays, ripples, resizing, and the
   camera layer composite in RGBA with Cairo, then convert to NV12. One
   decoder, one compositor, one encoder.
3. **Parallel ranges.** Exports at least eight seconds long, with at least six
   logical cores and non-passthrough content, split the frame plan into two to
   six contiguous ranges. Each range opens its own decoder, compositor, and
   encoder, so the per-frame work runs on separate cores. Ranges are joined
   with the concat demuxer and `-c copy`. The frame plan is unchanged: every
   frame appears exactly once, in order.
4. **Chunked hybrid (opt-in).** `APEXSHOT_EXPORT_HYBRID=1` cuts the timeline
   into short pieces and sends each piece to whichever encoder has the smaller
   backlog, mixing a hardware encoder and libx264. Pieces are joined the same
   way. It is off by default: on the measured machines the compositor, not the
   encoder, is the bottleneck, so splitting encode work adds process start-up
   and slower libx264 pieces without reducing wall time.

## Canvas

The background fill, padding, frame aspect (16:9, 9:16), and the video
card's rounded corners are laid on after the frame is rendered. The editor
renders the fill once at the canvas size and hands the export the canvas, the
centred video rect, and the corner radius (`CanvasLayout`). Every path above
then renders only the video rect, so passthrough and the GPU crop keep working
with a background, and `canvas::FramedSink` lays each frame onto the canvas:

- the fill is converted to NV12 once per range;
- each frame copies the background, then the video rows into the centre;
- only the rows that hold a rounded corner are blended, with a one-pixel
  anti-aliased edge, and chroma uses the average of its four luma samples.

A resolution pick scales the whole canvas: the video rect and the radius scale
with it, and both video edges stay even. The video rect's origin is rounded
down to an even pixel so its chroma lines up, which can leave the video one
pixel off centre.

The preview draws the cursor and the camera layer above the rounded clip; the
export rounds after them, so a cursor or camera bubble at the very corner is
clipped.

On the development machine, framing a 1080p passthrough export on a 2150×1310
canvas raised it from 7.0 to 7.8–8.4 ms/frame. A zoomed export on a fixed frame
is faster than without one, because the video rect is smaller than the source.

A zoom with a canvas moves and grows the card over the padding, as the
preview does. Frames whose card has left its rest rect take the CPU path:
the video rect is rendered as usual (cursor, ripple, camera layer), the
footage the card shows over the padding is cropped from the full decode, and
`Framer::compose_card` lays both over the zoomed background inside the card's
rounded shape in parallel row spans (8 ms for a 2160x1440 frame in release).
Resting frames keep the fixed-rect path.

## GPU stage

When the machine profile finds a hardware GPU the frame fits on, every frame is
composited on the GPU (`export/compose_gpu.rs`): the decoded NV12 is uploaded,
converted to RGB into a mipmapped texture, and one pixel shader places the source
(the crop stretched over the frame, or the card on a canvas), its rounded
corners, the zoomed background, the motion blur, ported from `motion_blur.rs`
at the studied 16 samples and 4-texel fetch span, and the click ripple, which
displaces each pixel's source position as `ripple_preview.rs` does. Two more
passes convert to NV12, and the planes are read back for the encoder. The cursor
is drawn on that frame inside its own box. Exports with a camera layer, and a
stage that fails mid-export, use the GPU crop and scale or the CPU compositor
below. `APEXSHOT_EXPORT_GPU_STAGE=0` keeps the CPU compositor.

In release on the development machine a blurred zoom transition renders in
5.2 ms at 1080p and 8.7 ms on a 2160x1260 canvas, against about 390 ms per
moving frame on the CPU.

`APEXSHOT_EXPORT_TRACE=1` logs, per range, the milliseconds per stage frame spent
waiting on the decoder, uploading, on the GPU, reading back, drawing the cursor,
and waiting for the encoder to take the frame. Tracing waits for the GPU after
each frame, so a traced export is slightly slower than an untraced one.

## GPU crop and scale

When an export needs a camera view — a zoom, or an output size different from
the source — and has no cursor, click ripple, or camera layer, the frame stays
in NV12 and D3D11's video processor crops and scales it. The CPU path converts
NV12 to RGBA, scales in software, and converts back; the GPU path uploads one
NV12 frame, blits it, and reads the result back, so both colour conversions and
the software scale disappear. Cursor and camera exports still use the CPU
compositor.

The scaler is best-effort. A missing D3D11 device, a driver without NV12 video
processing, BT.2020 colour, or `APEXSHOT_EXPORT_GPU=0` falls back to the CPU
path. A blit failure disables the scaler for the rest of the range and logs it.
Each parallel range opens its own device and processor, because a D3D11
immediate context is not thread-safe; at 1080p that is four textures, about
12 MB per range.

The video processor takes a whole-pixel source rectangle, so a slow zoom snaps
the crop to whole pixels. At 1080p that is one pixel. This path is not the
shared-surface ring yet: frames are read back to the CPU and written to ffmpeg.
Feeding the encoder a D3D11 surface directly is the next step.

On the development machine, a 1080p30 10-second 1.6× zoom export measured
15.6–16.2 ms/frame with the CPU compositor and 7.5 ms/frame with the video
processor (two ranges, Quick Sync), roughly twice as fast. A plain passthrough
export measured 7.4–8.3 ms/frame, so the zoom path now costs about what the
no-effects path does. A direct GPU-versus-CPU comparison of the same crop
measured 47 dB luma PSNR with a zero-pixel shift; real `testsrc2` output measured
38 dB overall because the two scalers filter differently.

## Encoder choice

`Encoder::fastest()` probes `h264_nvenc`, `h264_qsv`, and `h264_amf` once per
process by encoding two synthetic frames with the exact flags an export uses,
then falls back to `libx264`. A probe passes only when the driver can actually
run the encoder; an entry in `ffmpeg -encoders` is not enough.

The chosen codec is opened with a fixed H.264 profile and level for the output
size and rate (`-profile:v high`, `-level:v …`), and constant-quality rate
control at the editor's CRF. Every piece in a split export uses the same
profile, level, pixel format, frame rate, and size, so the pieces can be copied
into one file without re-encoding. Each piece starts on an IDR, which costs a
little size at the seams.

Rate control: x264 keeps the editor's CRF. NVENC, Quick Sync, and AMF get the
bitrate that CRF produces on the shared curve in `export/rate.rs` (capped at
1.5× with a 2× buffer), so a hardware export's size follows the dialog's
estimate instead of each driver's quantiser mapping. The dialog's quality tiers
are CRF 14, 24, 30, and 42.

If a hardware encoder fails at start or mid-export, the export retries once
from the beginning with `libx264`, matching the previous behaviour. The UI
progress bar resets because the retry starts over.

HEVC: the dialog's Codec row picks H.264 (default) or HEVC. HEVC tries
`hevc_nvenc`, `hevc_qsv`, then `hevc_amf`, falling back to `libx265`; every HEVC
encoder gets a bitrate from the tier's bits per pixel (0.3, 0.05, 0.0175,
0.0075) and the `hvc1` tag. The chunked two-encoder path stays H.264-only.

## Policy

- The export dialog has a **Hardware acceleration** checkbox, on by default.
  Turning it off forces `libx264`.
- `APEXSHOT_EXPORT_HARDWARE=0` forces software encoding without changing the
  dialog. This is the recovery switch for a driver whose encoder passes the
  probe but produces bad output.
- Decoding stays in software by default. `APEXSHOT_EXPORT_HWDECODE=1` decodes
  with D3D11 (`-hwaccel d3d11va`, falling back to software for a stream the GPU
  cannot decode), but copying each frame back for the filters made a 4K H.264
  decode three times slower (10 ms/frame against 3.4) on the development
  machine. It becomes worth enabling once decoded frames stay on the GPU.
- `APEXSHOT_EXPORT_WORKERS=1` disables parallel ranges. Values from 1 to 6 are
  clamped.
- `APEXSHOT_EXPORT_HYBRID=1` enables the chunked two-encoder path.
- `APEXSHOT_EXPORT_GPU=0` disables the D3D11 crop and scale path.

Unlike the Linux editor, Windows selects hardware automatically. The quality
caveat is the same: a CRF tier is passed to hardware encoders as a quantizer,
and that mapping is not validated across vendors and drivers. The checkbox is
the escape hatch for a user who compares outputs.

## Machines without a strong GPU

The export does not allocate D3D11 textures or shared surfaces yet, so nothing
in the current path reserves VRAM beyond what the driver gives the hardware
encoder. A machine with no usable hardware encoder runs `libx264`; a weak
machine is limited by the worker count:

- Fewer than six logical cores, or an export shorter than eight seconds, stays
  sequential.
- Outputs of 1440p and above use fewer parallel ranges; 4K uses at most two.
- Hardware encoders are capped at three concurrent ranges because some driver
  models limit concurrent encode sessions; `libx264` uses up to four.
- A hardware encoder that cannot open is skipped by the probe. A hardware
  encoder that fails later triggers the `libx264` retry.

The export reads one machine profile (`export/machine.rs`), logged at the start
of every export: cores, RAM and free RAM, and the GPU. Only a hardware adapter
counts as a GPU: with none, or only the software Basic Render adapter, the GPU
crop and scale is skipped and the CPU path runs. With several, the
low-power adapter is used, the integrated GPU on a hybrid laptop: frames are
uploaded and read back every frame, and an integrated GPU shares system RAM
with the CPU and the Quick Sync encoder. On the development machine a zoomed
1080p export ran 25-27 ms/frame on the Iris Xe and 26-97 ms/frame on the RTX
A500, which also slowed exports that never touched it by waking it.
`APEXSHOT_EXPORT_ADAPTER=high-performance` picks the other one. The GPU path also has to
fit: half the adapter's memory budget, and for an integrated GPU, which takes
its textures from system RAM, half the free RAM. Parallel ranges are capped so
they fit in half the free RAM (each range holds its frame buffers and its own
decoder and encoder). `APEXSHOT_EXPORT_RAM_MB=<n>` and `APEXSHOT_EXPORT_GPU=0`
simulate a weaker machine.

## Measuring

```
cd windows-ui
cargo test --lib export_throughput_benchmark -- --ignored --nocapture
APEXSHOT_EXPORT_HARDWARE=0 cargo test --lib export_throughput_benchmark -- --ignored --nocapture
```

The parity check between sequential and parallel ranges is an ignored
integration test:

```
cargo test --lib parallel_ranges_match_the_sequential_render -- --ignored --nocapture
```

The GPU scaler's crop, resize, and range-start behaviour are covered by the
ignored module tests:

```
cargo test --lib gpu::tests -- --ignored --nocapture
```

On the development machine (20 logical cores, 1080p, 30 fps, 10 s), the
passthrough path measured about 10 ms/frame with Quick Sync and about 9 ms/frame
with `libx264`, against roughly 25–31 ms/frame before passthrough. The zoomed
path measured about 16 ms/frame with one worker and 15.6 ms/frame with two on
Quick Sync; with `libx264` it went from 23.4 to 18.1 ms/frame. Those numbers are
a starting point, not a guarantee.
