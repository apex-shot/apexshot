# Windows capture reference

Status: research notes, 2026-11-09. Phase 1, display and area capture through
`ddagrab`, and Phase 2, window capture through Windows Graphics Capture, are
implemented. Everything after Phase 2 is still a proposal.

This is the reference for the Windows recording work: what the studied 4.1.0
build does, how ApexShot compares today, and the order in which the gaps should
be closed. It deliberately describes behaviour rather than naming the product
the behaviour was studied from.

## 1. The studied recorder

The studied build does not record through Electron's `desktopCapturer` or
`getDisplayMedia`. Its renderer talks to a native Swift service over a
JSON-RPC-like transport. The native service exposes dedicated process groups
for `recording` and `recordingInputs`, and the recorder itself is built on
ScreenCaptureKit:

- `SCStream`
- `SCContentFilter`
- `SCStreamConfiguration`
- one stream per source
- frames arrive as `CMSampleBuffer` and `IOSurface`
- output goes through `AVAssetWriter` and `SegmentedAssetWriterDelegate`
- `RecordingFinalizer` and `MetadataWriter` stitch the session together

The native binary also contains separate recorder classes for `Display`,
`Window`, `SystemAudio`, `Microphone`, `WebcamRecorder`, `CursorRecorder`, and
`FocusRecorder`. The source layout has separate directories for
`ScreenCaptureKitRecorders`, `RecordKitRecorders`, `DeviceRecorders`,
`UserInputRecorders`, `Output`, and `Session`.

### Capture modes

| Mode | How it is captured | Stored metadata |
|---|---|---|
| Display | Display content filter; native stream config sets width, height, `minimumFrameInterval`, `queueDepth`, `showsCursor`, and `pixelFormat` | Display id, bounds, scale |
| Area | Same display stream with `sourceRect` cropping | Display id plus crop rectangle |
| Window | Desktop-independent window filter with `WindowPlacement` and `contentRect` | Window id, bundle id, app name, title, border radius |
| External device | Separate device source | Device id |
| System audio | ScreenCaptureKit audio with `setCapturesAudio` and `setExcludesCurrentProcessAudio` | Audio track and selected process ids |
| Microphone | `RecordKitMicrophoneRecorder` or `LegacyMicrophoneRecorder`; `AudioHALDeviceObserver` and `AudioSilenceDetector` | Microphone track and device id |
| Webcam | `WebcamRecorder`; live preview through `SharedSurfaces` | Camera track, camera delay, aspect ratio |

The important architectural point is that a window is not captured as a screen
rectangle. The window filter captures the window's own content, so another
window covering it does not appear in the recording.

### Codec and frame rate

The native recorder's configuration carries a `VideoCodec` enum with two cases:
`H264` and `Hevc`. ProRes belongs to the export path (`ProResEncoding`), not to
recording.

The renderer has no recording-FPS setting. The native `SCStreamConfiguration`
sets `minimumFrameInterval`, so capture is paced from the display stream rather
than from a user choice. Export has separate FPS choices.

The exact default codec and bitrate could not be proven from the 4.1.0 main
process. That process is shipped as V8 bytecode (`dist-electron-index.x64.jsc`),
not readable JavaScript. The native symbols prove the enum and the presence of
the AVAssetWriter and VideoToolbox paths; they do not prove the selected
default.

### Audio and webcam

- System audio is captured natively, not through an Electron API.
- Selected-app system audio is supported by resolving bundle ids to process ids
  and passing them to `recordSystemAudio` as
  `{ type: "only-selected", processIds }`.
- Microphone capture is a separate native recorder, not the screen stream.
- Webcam capture is a separate native recorder. Live preview consumes native
  surfaces rather than recording frames.
- `cameraMaximumHeight` defaults to 1080.
- `cameraSync.measureCameraDelay` exists to measure camera latency.
- `cameraFaces.detectFaces` and `personMatte` exist for face tracking and
  person segmentation.

Recordings are segmented and recoverable. Unfinished recordings can be listed,
recovered, or discarded. Screen video, webcam, microphone, and system audio are
separate assets referenced by recording metadata.

## 2. Cursor, click, and autozoom model

### Recorded input

The native `MouseEvent` type carries:

- `kind`
- `timeMs`
- `position` (`CGPoint`)
- `cursorId` (optional string)

The renderer observes `mouseDown`, `mouseUp`, `mouseDragged`, and `mouseMoved`.
`CursorRecorder` records the events and `CursorTimeline` organises them. Cursor
images are stored in a separate cursor image directory and are referenced by
`cursorId`.

The project data also exposes:

- `mouseclicks`
- `mousemoves`
- `mousedowns`
- `mouseups`
- `keystrokes`
- `mouseEvents` (clicks and moves merged and sorted by recording time)

### Hover detection

Hover changes are computed natively by comparing pixels around the cursor.
`HoverDetector` tests whether a pixel change is explained by scrolling, so
scrolling does not create false hover events. Results are exposed as
`SessionHoverChanges { sessionIndex, changeTimesMs }`.

Hover times are used when `cursorConfig.followRecordedPositionOnHover` is on and
mouse smoothing is active. They act as sync points so the smoothed cursor does
not lag behind the app's real hover state.

### Sync points

The cursor path uses sync points with these tolerances:

| Event | Tolerance |
|---|---:|
| Click | 2 pt |
| Drag | 2 pt |
| Hover change | 4 pt |
| Cursor image change | 6 pt |

The sync points constrain the spring so smoothing cannot drift across a
meaningful event.

### Smoothing

Project animation configuration contains:

- `mouseMovementSpring` (nullable; null disables mouse smoothing)
- `screenMovementSpring`
- `motionBlurAmount` (default 1)

A spring has stiffness, damping, mass, and clamp. `lagMs` is
`damping / stiffness * 1000`. The target is sampled from the cursor path at
`time + spring lead`, and the simulation runs per integer millisecond. Runs of
positions are stored and `positionAt` interpolates between samples.
`instantRanges` disable smoothing for specific slices.

### Cursor defaults

```text
hide: false
size: 48
stopMovementInLastPartMs: 0
set: macos-tahoe
rotationMode: velocity
rotateOnXMovementRatio: 0.5
baseRotation: 0
useDefaultIfAppHasLowResolution: true
alwaysUseDefault: false
hideNotMovingAfterMs: null
loopPositionBeforeEndMs: null
removeShakeTreshold: 500
optimizeOriginalTypes: true
followRecordedPositionOnHover: true
clickEffect: null
```

Rotation modes:

| Mode | Values |
|---|---|
| `velocity` | `velocitySmoothingMs: 120`, `tiltDegPerVelocity: 12`, `maxTiltDeg: 20`, `startupCalmMs: 500`, `entryTeleportSpeedPxPerMs: 15`, spring stiffness 120, damping 8, mass 1 |
| `directional` | `alignRatePerPx: 0.00005`, `speedSaturationPxPerMs: 3`, `angularDampingMs: 100`, `maxTurnRateDegPerMs: 0.4`, `maxFromNeutralDeg: 135`, `minTravelPx: 4`, `neutralPullHeadingDeg: -112.5`, spring stiffness 320, damping 28, mass 1 |

### Click effects

Click effects are `ripple` or `circle`. Ripple has an optional `pullSize01`
(default 0.04). The GPU shader implements a ring wave, pull, chromatic
aberration, and a calm-zone fade. Circle mode uses radius and opacity geometry.

### Autozoom

Autozoom is created from click events. The rules are:

| Rule | Value |
|---|---:|
| Ignore clicks in the last | 1.0 s |
| Window start | `click time - 0.3 s`, floored at 0.001 s |
| Window end | `click time + 2.5 s`, capped at `duration - 0.8 s` |
| Merge gap | 2.5 s |
| Zoom scale | 2.0 |
| Type | `follow-click-groups` |
| Instant animation | false |

ApexShot already implements these constants in
`src/recording/editor/auto_zoom.rs:13-30` and computes focus at the midpoint of
the pointer movement inside the merged window, falling back to click positions.
That matches the studied behaviour. The editor's zoom math is not the gap.

## 3. Why the studied output looks smooth

1. **GPU capture instead of GDI.** Frames come from the compositor with native
   presentation timing, not from a BitBlt of the desktop.
2. **Hardware encode at native resolution.** Area capture uses a crop rectangle
   rather than capturing a larger surface and scaling it down.
3. **The cursor is not in the captured pixels.** It is a separate synthetic
   layer, which allows a larger cursor, spring smoothing, rotation, and click
   effects without a double cursor. The native config exposes
   `setShowsCursor`, and the editor always paints its own cursor, so the capture
   cursor is expected to be hidden. That last point is inferred, not directly
   proven from disassembly.
4. **Window capture uses a window filter.** Occluding windows do not appear.
5. **Camera, microphone, and system audio are separate tracks.** They cannot
   stall screen capture and are composited later.
6. **Zoom is an editor effect.** The lead, tail, merge, and end-margin constants
   make the movement feel intentional rather than reactive.

## 4. ApexShot today

### Screen and area

Screen and Area now use DXGI Desktop Duplication through the bundled
`ddagrab` filter. `capture-overlay/src/platform/windows/recording.cpp:629-704`
maps the requested region to one DXGI output and builds:

```text
-f lavfi -i ddagrab=output_idx=<n>:draw_mouse=<0|1>:framerate=<fps>
                :video_size=<wxh>:offset_x=<x>:offset_y=<y>
```

The D3D11 frames are downloaded before the existing scaler and encoder:

```text
hwdownload,format=bgra,scale=<w>:<h>
```

The output size, CRF, GOP, B-frames, `+faststart`, and hardware encoder probe
are unchanged. `h264_qsv`, `h264_nvenc`, and `libx264` remain the candidates.

`gdigrab` is still the fallback for Window recordings, for a region that leaves
or spans displays, and for machines where the `ddagrab` probe fails:

```text
-f gdigrab -framerate <fps> -draw_mouse 0
-offset_x <x> -offset_y <y> -video_size <wxh> -i desktop
```

GDI remains the quality and CPU bottleneck where it is used. Desktop
Duplication is compositor-backed, but this implementation downloads each frame
to system memory before encoding, so it is not yet a zero-copy path.

### Window

Window recordings now use Windows Graphics Capture with a window item instead
of a desktop rectangle. `window_capture.cpp` creates the item with
`IGraphicsCaptureItemInterop::CreateForWindow`, captures it with
`Direct3D11CaptureFramePool` and `GraphicsCaptureSession`, disables the captured
pointer, and writes paced BGRA frames to a named pipe. `recording.cpp` feeds
that pipe to ffmpeg as `rawvideo`.

Occluding windows are not part of the captured item. If Graphics Capture is
unavailable or its session cannot start, the recorder retries the segment with
`gdigrab`, which keeps the old rectangle behaviour as a fallback.

The remaining window limitation is resize handling: the output size is fixed
when the segment starts, so a window resized during recording is cropped or
letterboxed until the next segment. Window metadata beyond the rectangle is not
stored yet.

### Cursor

Cursor rendering is already synthetic and correct. Quick Access sets
`request.includeCursor = false` at
`capture-overlay/src/platform/windows/quick_access.cpp:2650-2656`, so recorded
pixels never contain a cursor that would double up with the editor's cursor.

The metadata is the weak point. `pointer_tracker.cpp`:

- starts a `QTimer` at `1000 / clamp(fps, 30, 60)`
- samples with `GetCursorPos` and `GetAsyncKeyState`
- records a click only if a poll happens while the button is down
- sets the click time to the sample time, not the real press time
- recognises standard Windows cursor handles only, with custom cursors mapped
  to `Default`
- has no hover-change or cursor-image-change data

The relevant regions are `:178-186` (timer and sample rate) and `:231-272`
(sampling and click detection). At 24 fps a quick click can fall entirely
between two samples, which loses the click for autozoom and click effects.

### Audio

ApexShot is already close here. `SystemAudioCapture` in
`capture-overlay/src/platform/windows/audio_devices.cpp:1060-1279` uses WASAPI
loopback and process loopback. Process loopback requires Windows 10 2004 or
later. The microphone uses the existing DirectShow path, and the tracks are
muxed after the recording stops.

The remaining work is timing and calibration, not basic capture support.

### Webcam

The camera records in its own ffmpeg process beside the screen process. The
track is camera-only: no desktop pixels, no pointer, and no audio. The preview
is a separate small MJPEG pipe for the camera bubble.

Capture settings:

- DirectShow mode selection prefers MJPEG up to 1080p, because raw modes above
  720p starve an integrated camera's USB bandwidth.
- The track is capped at 1920×1080 and encoded at CRF 18, independent of the
  screen quality tier.
- The camera runs at its own frame rate. The studied recorder's webcam config
  carries only `deviceId`, `maximumWidth`, `maximumHeight`, and codec settings;
  its `CameraCapture.selectFormat` chooses a format by size only, and
  `WebcamRecorder.deviceFrameRate` is separate from the display refresh rate.
  ApexShot follows that: it probes the camera's default format without pinning
  a rate, then encodes the track at the rate the device reports. A 60 fps
  desktop therefore records a 30 fps camera at 30, and a 60 fps camera at 60.

The editor contract is now the same for screen, area, and window recordings:

- The playable output video has the camera baked in.
- A hidden clean-screen file is kept beside it.
- The camera track and metadata stay in the pointer sidecar directory.
- The sidecar `webcam` block carries `file`, `width`, `height`, `duration`,
  `start`, the normalised `x`/`y`/`w`/`h`, `shape`, and `flip`.
- `start` is the gap from video time zero to the first camera frame. The
  recorder derives it from the camera track ending with the screen track.
- `screen_file` names the clean footage beside the video.

That lets the editor rebuild the camera layer from clean screen plus camera
track instead of replaying the baked-in camera. The fake-camera area test
produced a 7.4-second track starting at 0.367 seconds and retained the clean
screen file. A real 1920×1200 60 fps screen recording with an integrated camera
probed that camera at 1920×1080 and 30 fps, produced a 1280×720 30 fps camera
track, a 9.6-second track starting at 1.15 seconds, and a 10.75-second playable
file.

Still missing:

- Camera-delay calibration against the microphone or a visual sync event.
- Per-segment camera mapping across pause and resume. The current `start`
  describes the joined track, not each segment.
- Face tracking, person matte, crop window, and camera look controls. Those are
  editor-side work.
- The studied build also stores a manual `delayAdjustmentMs` and computes a
  camera delay from the webcam and microphone files. The sidecar has room for
  that adjustment, but the measurement is not implemented yet.

### Encoder

The Windows recorder uses H.264 with `-bf 2`, a GOP of `fps * 2`, and
`+faststart`. It does not use HEVC, and the probe does not consider AMF. On an
AMD machine without a working QSV or NVENC device, recording falls back to
`libx264`.

### Recording quality tiers

The three tiers do not change capture resolution or frame rate. They change the
encoder's constant-quality target:

| Tier | CRF / CQ |
|---|---:|
| Balanced | 23 |
| High (default) | 20 |
| Ultra | 16 |

`quick_access.cpp` passes `recordingCrfForQualityIndex()` into
`RecordingRequest::crf`. `buildScreenArguments()` subtracts the existing
resolution compensation, then applies the result as `-crf` for `libx264`,
`-global_quality` for QSV, or `-cq` for NVENC.

Measured on the window path at 1502×1000, 60 fps, five seconds each:

| Tier | File size |
|---|---:|
| Balanced | 289 KB |
| High | 319 KB |
| Ultra | 371 KB |

That is about 28% more data from Balanced to Ultra. Static text looks close at
all three; the difference shows up in motion, gradients, and fine detail. The
tiers are compression choices, not capture-quality choices.

Two caveats:

- The webcam composite used to re-encode every saved video at a fixed 16,
  which erased the tier whenever a camera was enabled. It now uses the selected
  tier minus two points for generation loss.
- On Linux, only `libx264`, NVENC, and VAAPI read `config.crf`. The VP9, VP8,
  and OpenH264 branches use fixed quality or bitrate, and VP9/VP8 are earlier
  in the encoder preference list. A Linux recording can therefore ignore the
  tier entirely unless those branches are mapped too.

### Editor

Autozoom, ripple/circle geometry, cursor themes, and the sidecar already exist.
The sidecar is version 2 and carries pointer samples, clicks, presses, webcam
layout, visibility, and capture region. The largest missing editor inputs are
hover changes, cursor-image changes, and accurate click timing.

## 5. Ranked gaps

1. **Click and cursor metadata are polled.** Fast clicks are lost, click times
   are imprecise, and hover and cursor-image changes are absent.
2. **Window capture has no live resize policy.** A resized window is cropped or
   letterboxed to the size the segment started with. Window metadata is still
   just a rectangle.
3. **No camera-delay calibration.** Camera and screen can be offset.
4. **Camera styling is not implemented.** The backend keeps the clean screen,
   camera track, layout, and start offset, but crop, shape, mirror, matte, and
   face tracking are editor work.
5. **Encoder coverage.** No HEVC recording and no AMF probe.
6. **The GPU path still downloads frames.** Display and area capture use
   `ddagrab`, but the encoder receives BGRA system memory. A cross-monitor area
   also falls back to `gdigrab`.
7. **Editor smoothing parity.** The constants exist, but the full two-spring
   model and every sync point may not be wired yet.

## 6. Implementation order

### Phase 1: display and area capture

Status: implemented for Screen and Area. `gdigrab` is still the fallback and
still handles Window recordings.

Why this is the right first slice:

- The bundled ffmpeg is 9.0.2 and already contains `ddagrab` and
  `scale_d3d11`.
- It is an input-argument change, not a new capture subsystem.
- It can be probed and rolled back per machine.
- It produces a measurable CPU and frame-pacing improvement without touching
  the editor or export code.

The bundled filter supports:

```text
output_idx, draw_mouse, framerate, video_size,
offset_x, offset_y, output_fmt, allow_fallback,
force_fmt, dup_frames
```

`offset_x` and `offset_y` are relative to the selected output. A local test
captured the correct 640×360 crop at offset 400,200, and encoded a 1280×720 clip
through `hwdownload,format=bgra` and `libx264`.

A local 5-second 1080p60 `-f null` benchmark reported:

| Input | CPU time |
|---|---:|
| `gdigrab` | about 1.70 s |
| `ddagrab` plus `hwdownload` | about 0.48 s |

The machine matters, and a null output does not include the encoder. Treat the
numbers as directional evidence, not a product guarantee.

What landed:

- `capture-overlay/src/platform/windows/ddagrab.{h,cpp}` enumerates DXGI
  outputs, maps a desktop rectangle to one output, and builds the filter
  string.
- Output matching uses `DXGI_OUTPUT_DESC.DeviceName` against
  `Platform::MonitorInfo.name`, with a bounds fallback for drivers that report
  a different name.
- `offset_x` and `offset_y` are output-local; `video_size` is the requested
  crop. A region that leaves or spans a display is rejected and falls back to
  `gdigrab`.
- `ddagrabSupported()` probes the capture and `hwdownload` chain before the
  recorder uses it. The existing hardware encoder probe still chooses between
  `h264_qsv`, `h264_nvenc`, and `libx264`.
- `-draw_mouse` follows `request.includeCursor`. Quick Access still passes
  false, so the editor's synthetic cursor is the only one in the result.
- Window recordings still use `gdigrab`; Phase 2 replaces that with a real
  window filter.

Observed on this machine:

- The capture overlay and all three related test targets build.
- `apexshot-ddagrab-tests` passes, including a DPI-aware live check that the
  DXGI bounds match `Platform::MonitorInfo.geometry` at 1920×1200.
- `apexshot-recording-tests` passes. Its log shows
  `capture: ddagrab output=0 offset=24,24 size=320x240`, and its output is a
  valid 320×240, 30 fps H.264 MP4.
- `apexshot-webcam-preview-tests` passes after the real
  `keepTopmost`/`excludeFromCapture` implementations replaced its local stubs.
- A full-screen 1920×1200, 30 fps, 10-second `ddagrab` clip encoded through
  `h264_qsv` (`global_quality 20`) produced 300 valid frames at about
  760 kbps. The content was mostly static, so that bitrate is not a motion
  quality result.
- A top-level window marked `WDA_EXCLUDEFROMCAPTURE` did not appear in a
  `ddagrab` frame, so Desktop Duplication honours the same exclusion the
  recording controls already set.

Still to validate:

- 1080p60 and 4K30 for at least ten minutes each.
- Multi-monitor and mixed-DPI displays, including a cross-monitor area.
- Hardware-accelerated app content, including browser video and a game.
- Pause, resume, stop, discard, and A/V sync on the new input.
- CPU usage compared with the current `gdigrab` path over a full recording.
- Fallback path with `ddagrab` disabled.
- Direct D3D11 encoder interop. The current path downloads to BGRA first, so
  the hardware encoder consumes system memory.

### Phase 2: window capture

Status: implemented.

`capture-overlay/src/platform/windows/window_capture.{h,cpp}`:

- creates a `GraphicsCaptureItem` with
  `IGraphicsCaptureItemInterop::CreateForWindow`
- uses `Direct3D11CaptureFramePool::CreateFreeThreaded` and
  `GraphicsCaptureSession`
- sets `IsCursorCaptureEnabled(false)`, because the editor paints the cursor
  from the sidecar
- copies the captured D3D11 texture to a staging texture and writes paced BGRA
  frames to a named pipe
- repeats the newest frame when the window is static

`recording.cpp` starts the pipe source after ffmpeg opens the pipe, reads it as
`rawvideo`, and stops the source on pause, stop, discard, or failure. If the
source cannot start, the segment is retried with the old `gdigrab` rectangle.

The output size is fixed at segment start. A window resized while recording is
cropped or letterboxed rather than restarting the segment. Window-relative
pointer coordinates, title/app metadata, and recorded border radius are not
stored yet.

Observed on this machine:

- `apexshot-window-capture-tests` creates a visible 320×240 Win32 window,
  captures 10 frames through the named pipe, and verifies the center BGRA
  colour.
- A 30-second, 60 fps window recording of a 1502×1001 window produced a
  1502×1000 H.264 clip with 1784 frames and 29.73 seconds of video. The
  0.27-second shortfall is the capture startup latency.
- A pause/resume window recording produced a valid 2.42-second clip from a
  4-second run, so the WGC source restarts cleanly for a new segment.
- A restart test discarded the first window take and recorded a fresh
  1502×1000, 60 fps, 2.72-second clip from the second take.
- A full-screen 1920×1200, 60 fps pause/resume run produced a valid
  2.10-second clip from a 2.8-second run.

A window is captured at its native pixel size. Playing a 1502×1000 window
recording full-screen on a 1920×1200 display upscales it by about 1.28×, so
some softness is expected. Recording the display instead keeps the full
1920×1200 resolution.

Still to validate:

- A window covered by another window for most of the recording.
- A window moved and resized during the recording.
- Minimise and restore.
- Rounded corners and high-DPI scaling.
- A window on a secondary monitor.

### Phase 3: cursor and click accuracy

Replace polling for button events with a low-level mouse hook
(`WH_MOUSE_LL`) or Raw Input. The hook should capture:

- button down and up
- drag state
- exact position at the event
- event time mapped onto the recording media clock

Keep polling for cursor shape, visibility, and fallback position. Extend the
sidecar:

- cursor id and image changes
- hover-change times
- cursor visibility intervals
- window-relative coordinates for window recordings

Add a camera-independent hover detector that compares pixels around the cursor
and skips changes explained by scrolling.

Validation:

- Twenty rapid clicks, including clicks shorter than one frame interval.
- Drags across the drag threshold.
- Clicks during pause and immediately after resume.
- 24, 30, 50, and 60 fps recordings.
- Window and area recordings.
- Click count and timestamps compared with an independent hook log.

### Phase 4: camera and audio sync

Add a camera-delay measurement equivalent to the studied
`cameraSync.measureCameraDelay`. Record camera frame timestamps and store the
measured offset so the editor can align the camera track.

Verify microphone and system-audio offset with a clap or flash test. The
capture paths already exist; this phase is about calibration and drift.

### Phase 5: editor parity

Wire the remaining studied behaviour into the editor:

- the two-spring model (`mouseMovementSpring` and `screenMovementSpring`)
- click, drag, hover, and cursor-image sync points
- velocity and directional rotation modes
- motion blur amount
- hover-aware cursor following

ApexShot's autozoom constants already match. This phase is about making the
cursor path use the new metadata.

## 7. Validation commands

For capture-overlay changes:

```powershell
cmake -S capture-overlay -B capture-overlay/build `
  -G Ninja -DCMAKE_BUILD_TYPE=Release `
  -DCMAKE_PREFIX_PATH=C:/Qt/5.15.2/msvc2019_64 `
  -DAPEXSHOT_CAPTURE_BUILD_TESTS=ON
cmake --build capture-overlay/build
ctest --test-dir capture-overlay/build --output-on-failure
```

For Rust changes:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets
cargo test
```

For Windows UI changes, follow `AGENTS.local.md`: rebuild and relaunch through
Cargo from `windows-ui`, leave it running, and keep build parallelism at or
below four.

## 8. Evidence and limits

The study used:

- deobfuscated renderer chunks
- the native recorder binary's class names, symbols, source paths, and string
  tables
- the 4.1.0 package metadata and bundled binaries

Limits:

- The 4.1.0 main process is V8 bytecode, so main-process logic could not be read
  as JavaScript. The older study tree has a plain-JS main bundle if deeper
  main-process details are needed.
- The exact recording codec, bitrate, and frame rate are not proven. The codec
  enum, the ScreenCaptureKit configuration setters, and the absence of a
  renderer FPS field are proven.
- The capture cursor is expected to be hidden because the editor always paints
  a synthetic cursor and the native config exposes `setShowsCursor`. The actual
  boolean value was not read from disassembly.
- The `ddagrab` benchmark was run on one Windows machine with the bundled
  ffmpeg. Hardware encoder interop varies by GPU and driver.
- `ddagrab` captures one DXGI output. Cross-monitor area behaviour still needs
  a product decision.

## 9. Open questions

1. Should a window resize restart the segment, change the output dimensions, or
   keep the current fixed-size crop/letterbox policy?
2. Should window recordings store title, app name, and border radius in the
   sidecar, or keep only the rectangle?
3. Should a cross-monitor area be supported, clamped to one monitor, or
   composed from multiple outputs?
4. What is the minimum supported Windows build for WGC, process loopback, and
   the chosen encoder set?
5. Should recording move to HEVC, or stay H.264 for editor and decoder
   compatibility?
6. Should camera start be measured from capture timestamps or from a
   content-based delay against the microphone, as the studied build does?
7. Should the camera path move from DirectShow to Media Foundation, or keep
   ffmpeg and add calibration only?
8. How should hover detection work when the screen is static but the app's
   hover state changes without a pixel change?
