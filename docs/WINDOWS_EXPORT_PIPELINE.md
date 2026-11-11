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

## Policy

- The export dialog has a **Hardware acceleration** checkbox, on by default.
  Turning it off forces `libx264`.
- `APEXSHOT_EXPORT_HARDWARE=0` forces software encoding without changing the
  dialog. This is the recovery switch for a driver whose encoder passes the
  probe but produces bad output.
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

When a GPU compositor lands, its frame ring can be sized from
`IDXGIAdapter3::QueryVideoMemoryInfo` and the same range split can fall back to
the CPU compositor on WARP or low-memory adapters.

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
