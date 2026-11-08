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

On the development machine (20 logical cores, 1080p, 30 fps, 10 s), the
passthrough path measured about 10 ms/frame with Quick Sync and about 9 ms/frame
with `libx264`, against roughly 25–31 ms/frame before passthrough. The zoomed
path measured about 16 ms/frame with one worker and 15.6 ms/frame with two on
Quick Sync; with `libx264` it went from 23.4 to 18.1 ms/frame. Those numbers are
a starting point, not a guarantee.
