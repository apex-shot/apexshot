# Windows Recording Plan

Status: in progress. The recorder seam, the ffmpeg-backed Windows backend, and
the Quick Access integration below are implemented in this branch. The deferred
items at the end are not.

## Goal

Give the Windows build video recording without touching Linux behaviour. The
Windows surface is the Quick Access shell (`platform/windows/quick_access.cpp`);
the Linux overlay keeps emitting a recording request to the Rust recorder, and
no Linux code path changes.

## What exists

- Windows capture, window enumeration, monitors, OCR, and QR already sit behind
  `platform/platform.h` with implementations in `platform/windows/`.
- The Quick Access shell already owns display, area, crosshair, window, timer,
  and OCR capture, and it already reuses `CaptureModeToolbar`.
- `CaptureModeToolbar` already has a Video toggle and reports
  `Result::recording`, `Result::microphone`, and `Result::speaker`. Quick Access
  passed `recordingAvailable = false`, so the item was always disabled.
- The Rust recording stack is Linux-only (PipeWire + GStreamer + X11) and the
  Rust crate cannot build for Windows yet, so it is not a foundation for this
  step.

## Decision: ffmpeg drives capture and encode

The recorder spawns a bundled `ffmpeg.exe` instead of linking Media Foundation.

Why:

1. The repo already treats ffmpeg as the encoder on the Wayland path, so the
   quality tiers, CRF mapping, and MP4 output stay conceptually identical.
2. `gdigrab` captures any desktop rectangle and draws the cursor, which maps
   directly onto the display, area, and window rectangles the shell already
   computes.
3. Audio, H.264, and MP4 muxing are one process, with no COM lifetime or
   hardware-encoder matrix to maintain in C++.
4. A future `ddagrab` (DXGI Desktop Duplication) upgrade is a source-argument
   change, not a new subsystem.

Trade-off: the Windows package has to ship an ffmpeg binary. The recorder
locates it in this order: `APEXSHOT_FFMPEG`, next to the executable, a `bin/`
subdirectory, then `PATH`.

## Seam

`src/platform/recording.h` is the platform seam. It contains no Windows
headers:

```text
RecordingRequest   target, region, window id, output path, cursor, audio,
                   fps, resolution cap, CRF
RecordingSession   start / pause / resume / stop / discard
                   elapsedChanged / stateChanged / finished(kept, path, error)
```

`platform/windows/recording.cpp` is the only implementation. It is only linked
into the Windows target, so the Linux target keeps its current shape. The
Quick Access shell is Windows-only, so no portable file gains a platform
`#ifdef`.

## Pipeline

Capture:

- `-f gdigrab -framerate <fps> -draw_mouse <0|1>
  -offset_x <x> -offset_y <y> -video_size <wxh> -i desktop`
- Display: the monitor's device-pixel rectangle.
- Area: the area selector's selection mapped from frozen-image pixels into
  desktop pixels (`frozen.geometry.topLeft() + selection`).
- Window: the window's visible frame. This is a rectangle capture, not an
  occlusion-free window capture; see the deferred list.

Encode:

- `libx264`, `veryfast`, `yuv420p`, output frame rate equal to the capture rate.
- Frame rate, resolution cap, and quality tier come from the recording settings
  described below. The quality tier selects the CRF base (Balanced 23, High 20,
  Ultra 16), and smaller outputs get the same resolution compensation as Linux
  (up to -10 CRF below a 2000 px diagonal).
- Dimensions are forced even; an optional maximum resolution uses
  `force_original_aspect_ratio=decrease` and never upscales.
- `-movflags +faststart` so the file plays before it is fully copied.

Audio:

- Microphone: the first dshow audio input, preferring a name containing
  "microphone".
- Speaker: only when a dshow loopback device exists (`Stereo Mix`,
  `virtual-audio-capturer`, `CABLE Output`). ffmpeg's dshow input has no WASAPI
  loopback of its own, so a missing device means the track is omitted rather
  than fabricated.
- `aac`, 160 kbps; stereo, or mono when the setting asks for it.

Pause and resume:

- ffmpeg cannot pause an encoder, so a pause finalises the current segment with
  `q` on stdin and a resume starts the next segment with identical settings.
- Stop concatenates the segments with the concat demuxer (`-c copy`) and adds
  `+faststart` to the final file.
- Discard kills any running encoder and removes every segment and the final
  file.

The work directory is `%TEMP%/apexshot-recording-<pid>-<timestamp>/`, removed
after a successful save or discard.

## Recording settings

The recorder reads the Linux Settings → Video values from QSettings. Each one
is an option index with the same table as its Linux config field, so a value
keeps its meaning across platforms:

| Setting | QSettings key | Values |
|---|---|---|
| Frame rate | `recording/video_fps` | 0 = 24, 1 = 30, 2 = 50, 3 = 60 |
| Maximum resolution | `recording/video_max_res` | 0 = Original, 1 = 1080p, 2 = 720p, 3 = 1440p, 4 = 900p, 5 = 480p, 6 = 2160p |
| Recording quality | `recording/video_quality` | 0 = Balanced, 1 = High, 2 = Ultra |
| Mono audio | `recording/video_mono` | bool |

`platform/recording_settings.h` owns the tables, the index-to-value mappings,
the resolution fit, and load/save. Missing or out-of-range values read as the
defaults (30 fps, Original, High, stereo), so a stale or hand-edited config
cannot reach the encoder. Quick Access deliberately has no controls for these
yet; the settings surface will write the same keys through
`saveRecordingSettings`.

## Quick Access integration

- `openMenu` asks `RecordingSession::available()` and passes that to
  `CaptureModeToolbar::choose`. The Video item is enabled only when ffmpeg and
  `gdigrab` are present.
- The toolbar's `recording`, `microphone`, and `speaker` flags flow into
  `CaptureOptions`; display, area, and window then start a recording instead of
  presenting a still.
- A recording countdown runs after the source is chosen, so area and window
  selections finish first. It is the large centred circle from the Linux
  recording flow: the remaining seconds, an accent ring that drains, and a
  click-to-cancel state. The tray Record submenu offers Off, 3, 5, and 10
  seconds; the choice is persisted with QSettings and defaults to 3 seconds.
- The tray gains a Record submenu (Screen, Area, Window) so recording does not
  require toggling Video inside the capture toolbar.
- A segmented recording bar follows the Linux controls: an inset stop/timer
  segment in the app's danger red, then pause/resume and discard separated by
  hairline rules. It stays above other windows with `Platform::keepTopmost` and
  is marked `WDA_EXCLUDEFROMCAPTURE`, so the recording does not contain its own
  controls.
- Area and window recordings dim the rest of the desktop through a
  click-through, capture-excluded mask, leaving the recorded rectangle clear
  with a thin accent border.
- The bar prefers the dimmed space below the selection, then above it. When
  neither side fits it moves inside the recording at the top, so the bottom of
  the area stays readable.
- On save, the existing result card shows the capture thumbnail, dimensions,
  file size, Open, and Show in folder.

## Verification

Build on Windows:

```powershell
. C:\dev\tools\devshell.ps1
cmake -S capture-overlay -B capture-overlay/build `
  -G Ninja -DCMAKE_BUILD_TYPE=Release `
  -DCMAKE_PREFIX_PATH=C:/Qt/5.15.2/msvc2019_64
cmake --build capture-overlay/build
```

With `-DAPEXSHOT_CAPTURE_BUILD_TESTS=ON`, `ctest` runs the recording-settings
table and persistence checks, plus the ffmpeg smoke test (skipped when ffmpeg is
not installed).

Manual pass:

1. Record a display, an area, and a window for a few seconds each.
2. Pause for several seconds, resume, stop, and play the result: the paused
   span must not appear, and audio must stay in sync.
3. Toggle the cursor off and confirm it is absent.
4. Record with the microphone; confirm one audio track.
5. Discard a recording; confirm no `.mp4` and no work directory remain.

Linux invariant:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --lib
cmake -S capture-overlay -B capture-overlay/build && cmake --build capture-overlay/build
```

The only shared file touched is
`CaptureOverlay_DrawingPrimitives.cpp` (an `M_PI` fallback). The Linux target's
source list is unchanged.

## Deferred

- **True window capture.** The rectangle path records whatever is on screen in
  the window's frame, including windows on top of it. Windows.Graphics.Capture
  with a window item is the follow-up.
- **System audio without a loopback device.** Needs WASAPI loopback, either
  native or an ffmpeg build that exposes one.
- **Hardware encoders.** `h264_nvenc`, `h264_qsv`, and `h264_amf` should be
  probed and offered as profiles, mirroring the Linux encoder selection.
- **`ddagrab`.** Faster per-output capture once the DXGI output-to-monitor
  mapping is settled.
- **Restart and delete-last-segment** controls, now that segments exist.
- **Video editor, cursor track, and click effects.** Those live in the Rust
  editor and wait for the Rust crate to build on Windows.
- **Recording settings surface.** Frame rate, resolution cap, quality tier, and
  mono are persisted and applied, and the Rust settings window now edits them.
  Output folder and filename pattern remain fixed at Videos/ApexShotRec and the
  timestamped default there; the settings page exposes them and the shell reads
  them from `settings.json`.
