# Hardware encoding in the editor export

The editor export encodes with `libx264` by default. A working VA-API or NVENC
encoder can be selected explicitly with `APEXSHOT_EXPORT_HW_ENCODER`; this note
records why the switch is opt-in rather than automatic.

## What is detected

`src/recording/editor/hardware_encode.rs` probes each candidate by actually
encoding a few synthetic frames (`-f lavfi … -f null -`) with the flags an
export would use, and checking the exit status. Listing an encoder in
`ffmpeg -encoders` and seeing a `/dev/dri/renderD*` node only proves the code
is compiled in; the probe proves the driver can run it. The GPU vendor is never
consulted — an NVIDIA machine without working NVENC and an Intel machine with a
missing render node both fail the same way.

VA-API needs `-vaapi_device <node>` and a trailing `format=nv12,hwupload` in the
filter graph. NVENC reads system-memory frames and needs neither.

## Why it is not automatic

- **Rate control does not match.** libx264 exports use CRF; VA-API and NVENC use
  a constant quantizer (CQP). The probe passes the tier's CRF through as the QP —
  a starting point, not a validated equivalence. The same "High" export can be
  larger or softer on hardware.
- **There is no single consistent hardware encoder.** On Linux, VA-API and NVENC
  differ from each other and across driver generations, so the result depends on
  the machine, not just on the quality tier the user picked.
- **The user picked a tier, not an encoder.** Silently changing the final file
  based on the GPU is the kind of surprise that is hard to explain afterwards.

The default export is therefore byte-for-byte the `libx264` output it always
was. A requested encoder that fails the probe falls back to `libx264` with a
note rather than failing an export that would otherwise have finished.

## Evaluating it

```
APEXSHOT_EXPORT_HW_ENCODER=nvenc|vaapi|auto <app> …
```

`auto` uses whatever the probe finds. Compare an export at each tier against the
`libx264` baseline — size, and the grain on flat backgrounds and text. If the
quality holds, flipping the default is a one-line change in
`selected_export_encoder`.

## Risks before this becomes the default

- The CQP/CRF mapping is unmeasured; the numbers should come from a real
  side-by-side comparison first.
- VA-API uploads at the end of the composite graph, adding a format conversion.
  Only the argument shape is covered by tests here; a live encode is needed to
  confirm there is no chroma shift.
- Hardware encoders change between driver releases, so "works" is per-machine
  and per-boot, not a property of the GPU model.
