# GPU cursor compositing (scoping note)

Status: **proposal only, nothing implemented.** This note records the parity
gap, a recommended path, and the risks, so the work can be picked up or dropped
deliberately.

## The parity gap

Screen Studio draws the cursor as part of the final composite: the capture
carries no pointer, the recorder stores a compact position/kind track, and the
compositor draws a sharp sprite at the output resolution with a shadow and a
motion trail. ApexShot already stores the same kind of track (`PointerSidecar`),
but composes the cursor on the CPU: one full-canvas RGBA frame per output frame,
rendered with Cairo and streamed to ffmpeg (`cursor_track.rs` →
`cursor_export.rs::write_rgba_track`), then blended with `overlay=0:0`
(`build_composite_convert_args` in `ffmpeg.rs`).

At 1080p that is 8.29 MB per frame of raster work and pipe traffic, most of it
transparent pixels, and `write_rgba_frame` walks every pixel to un-premultiply
and re-order to RGBA. The export is correct, but its cost scales with canvas
area rather than with the cursor.

## Where it runs today

| Piece | File |
| --- | --- |
| Cursor track streamed on `pipe:4` | `cursor_track.rs`, `cursor_export.rs` |
| Sprite bitmaps, hot spots, click effects | `cursor_sprite.rs`, `assets/cursors/` |
| Full-canvas overlay blend | `build_composite_convert_args` in `ffmpeg.rs` |
| Ripple warp (GPU, ripple only) | `gst_warp.rs` |

## Options

### A. Shrink the overlay and drive it with `sendcmd`

Keep the CPU renderer, but rasterize only the cursor's bounding box and place it
with `overlay=x:y` driven by a `sendcmd` file — the mechanism the zoom camera
already uses (`build_sendcmd` / `zoom.cmd`).

- The frame on `pipe:4` shrinks from `W×H` to the sprite rect, so the Cairo work
  and the pipe bandwidth fall by roughly the area ratio.
- `overlay` accepts `x`/`y` as runtime commands, so placement needs no change to
  the graph beyond replacing the constant `0:0`.
- `overlay=alpha=premultiplied` plus cairo's premultiplied `ARGB32` (BGRA byte
  order on little-endian) could remove the per-pixel un-premultiply loop in
  `write_rgba_frame`. **Verify before relying on it**: confirm ffmpeg's
  `bgra`/`argb` pixel-format byte order and that `premultiplied` matches what
  cairo writes.
- Caveat: the click-ripple ring is a large fraction of the video width, so the
  sprite bound is not always small. The win is best for the cursor-only case;
  the ring is transient.

This stays a CPU path. It cuts cost but does not give Screen Studio's
output-resolution sprite, because the sprite is still rasterized at one scale
and overlaid.

### B. Composite the sprite on the GPU

Extend the GL stack that already runs for the ripple (`filesrc → decodebin →
videoconvert → glupload → glshader → gldownload`) into a two-input compositor
that draws the cursor sprite into the frame.

Needed:

- **A second input.** `glshader` has a single `sampler2D`; a cursor needs a
  second texture. This means a true two-input element (`glvideomixer`,
  `glmixer`) or a custom compositor, not the existing shader.
- **Per-frame uniforms.** `uniform_state` already uploads 16 floats per frame;
  position, scale, alpha, and an atlas index/UV rect are a small extension.
- **A sprite atlas.** Kind and scale change per frame, so pack the nine themes ×
  four kinds (36 PNGs) into one texture and select a sub-rect per frame.
- **The click effects.** The ripple is already a footage displacement in the
  shader. Spotlight/Echo/Circle are vector overlays and need either shader math
  or prerendered sprites.
- **A pipeline for every composite export.** Today the GL pipeline runs only
  when the ripple is visible (`WarpSetup::from_state` returns `None` for other
  click effects). Cursor compositing has to run whenever a cursor is drawn, so
  the export graph changes shape for the common case. This is the biggest risk.
- **A CPU fallback.** `gl_warp_available()` gates on `glupload`/`glshader`/
  `gldownload`; the compositor inherits that gate and must fall back to A (or
  the current full-canvas path) when GL is missing.

## Recommendation

**Stage it: A first, B as the parity target.**

A is small, keeps the working ffmpeg export, removes the dominant cost (the
full-canvas per-pixel loop) on the 4 GB machine the export path is being
hardened for, and proves the premultiplied-alpha assumption B needs.

B is the actual parity work. It should follow only after A has landed and
two-input GL element availability has been confirmed across the supported
GStreamer versions (1.20+ on Ubuntu 24.04 / Arch). Build B behind the existing
GL gate with a fallback to A, so a machine without GL keeps exporting.

## Risks and open questions

- **Two-input GL element support.** Confirm `glvideomixer`/`glmixer` ship in the
  packaged `gst-plugins-bad` on every supported distro and behave like
  `glshader` for texture targets. If not, B needs a custom element — a much
  larger change.
- **Ripple vs cursor interaction.** The warp skips the drawn ring when it
  displaces the footage (`skip_ripple_ring`). A GPU compositor drawing the
  cursor in the same pass must keep that rule or double the ripple.
- **Motion smoothing lives in Rust.** `CursorMotion` (smooth / hide-idle /
  speed) and press scaling are computed per frame in `cursor_export.rs`. The GPU
  path must keep computing them as uniforms, or move them into the shader.
- **Alpha and colour.** A frame-for-frame visual diff against the current
  overlay is required before switching. The chroma cast across zooms caused by
  an RGB round-trip is a known failure mode (see the comments in
  `build_composite_convert_args`).
- **Performance is unmeasured.** The "GPU is faster" claim needs a before/after
  on the target machine (CPU, peak RSS, export wall time), not an assumption.
- **Scope.** B is a new compositor. If A already meets the visual bar, B may not
  be worth its risk.

## Verification checklist (for whoever implements)

1. Land A; diff a cursor-only export and a ripple export frame-by-frame against
   the current output at 1080p and 4K.
2. Confirm the premultiplied path matches the un-premultiplied one byte-for-byte.
3. If B is attempted, build it behind `gl_warp_available()` and prove the
   non-GL fallback still exports.
4. Measure export wall time, peak RSS, and pipe throughput on a 4 GB machine.
