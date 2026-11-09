# Autozoom reference

Status: research notes, 2026-11-09. This describes how the studied 4.1.0-5005
build creates and animates automatic zooms, then compares that behaviour with
ApexShot's generator and Windows editor camera.

This document deliberately describes behaviour without naming the product it
was studied from. "The studied build" means the 4.1.0-5005 snapshot. The
renderer chunk aliases below come from that snapshot; they are content-hash
names and carry no product meaning.

## 1. Evidence and scope

The study used the deobfuscated renderer chunks from the 4.1.0-5005 snapshot.
The application's main process ships as V8 bytecode, so main-process project
construction and any import-time seeding are not covered here. Everything
below is renderer-side behaviour read from source. No frame-level or
performance measurements were taken.

| Alias | Chunk | Role |
|---|---|---|
| `DXX` | `DXXvn2Sd2` | Zoom model, defaults, automatic-range generator |
| `B39` | `B39sgn5K` | Pointer path, target selection, framing, screen facts, spring binding |
| `YEi` | `YEiLxxgG` | Collection helpers, time lookup, rectangle math |
| `DgH` | `DgH-sR-Y2` | Project and zoom configuration, layout resolver |
| `CpX` | `CpXpNcFh` | Spring defaults and integrator |
| `BbP` | `BbPLqgNs2` | Add-zoom action |
| `CK1` | `CK1CPV1g2` | Last-edited zoom style |
| `DSY` | `DSYc1gpA` | "Restore default zoom ranges" command |

## 2. The two machines

Autozoom is split into two independent stages:

```text
click stream
    -> source-time ranges
    -> zoom timeline items

recorded pointer path + one zoom item's range
    -> movement groups
    -> active group centre
    -> target screen rectangle
    -> spring-driven screen rectangle
```

The range generator never chooses where the camera looks. It only decides when
a zoom exists. The focus is a pure function of the current source time and the
recorded pointer path, recomputed while the timeline is evaluated.

A zoom item carries:

```text
id
sourceStartMs, sourceEndMs
zoom                   scale, default 2
type                   "follow-click-groups" or "manual"
presentation           "screen" or "loupe"
loupeOptions
manualTargetPoint      undefined for automatic zooms
isDisabled
hasInstantAnimation
```

`isAuto` is defined as `type !== "manual"` (`DXX:3259-3264`). The item's
minimum duration is 100 ms (`DXX:3147-3155`). All fields are serialised, so a
zoom stays attached to its source footage through trimming and retiming.

## 3. Ranges from clicks

The generator is `DXX:1248-1263`. In pseudocode:

```text
if recording.mode == "externalDevice":
    return []

for e in recording.mouseclicks where e.recordingTimeMs < durationMs - 1000:
    t = floor(e.recordingTimeMs)
    range = [ max(t - 300, 1), min(t + 2500, durationMs - 800) ]

merge ranges while next.start - last.end <= 2500

for each merged range:
    emit Zoom {
        sourceStartMs: floor(start),
        sourceEndMs: floor(end),
        zoom: 2,
        type: "follow-click-groups",
        presentation: "screen",
        hasInstantAnimation: false
    }
```

The constants are `ZOOM_LEVEL = 2`, `before = 300`, `after = 2500`, and
`fillGap = 2500` (`DXX:1215-1221`, `DXX:1258`).

Details that are easy to miss:

1. **The input is the whole `mouseclicks` stream, not only presses.** The
   recording model's `mousedowns` and `mouseups` are both filters over
   `mouseclicks` (`YEi:2276-2287`). A normal click therefore contributes two
   ranges, one anchored on press and one on release, which normally merge. The
   tail follows the release, not the press.
2. **The merge gap is between ranges, not between clicks.** Each range already
   extends 2.5 s after its click and 300 ms before it. Two separate clicks
   therefore merge when they are up to 5.3 s apart. ApexShot's generator tests
   already encode that boundary: 3.0 s and 8.3 s merge, 3.0 s and 8.4 s do not.
3. **The bounds are source-time clamps.** A click on the first frame still
   leaves a 1 ms sliver of full-frame footage. Clicks at or after
   `duration - 1000` are ignored. The window never reaches the last 800 ms.
4. **The collection enforces the 100 ms minimum on insertion.** The generator
   does not filter short windows itself. `replace` removes all items and adds
   the generated ones through `add`, which fits each item into a free range and
   applies the minimum duration (`DXX:2775-2780`).
5. **Nothing here analyses content.** There is no scroll detection, no window
   detection, no click-type discrimination, and no semantic grouping. The only
   inputs are click times and the recording duration.

The generator is called by the "Restore default zoom ranges" command
(`DXX:3661-3663`, command at `DSY:208-216`). I did not find an import-time
caller in the renderer. A fresh project may be seeded in the bytecode main
process, so that path is unproven.

## 4. The focus path

This is the part that gives the result its authored feel.

### 4.1 Build the pointer path

`cs` takes `recording.mouseEvents`, shifts positions into crop coordinates, and
applies `removeShakeTreshold` (`B39:3274-3284`). `ls` then:

- trims the path to source intervals still present in playback;
- moves the first sample to the playback start;
- when `loopPositionBeforeEndMs` is set, appends a copy of the first position
  1 ms after the last event to close the loop;
- otherwise returns the trimmed path unchanged.

See `B39:3285-3309`. Trimming therefore changes the framing path, not just the
timing.

### 4.2 Decimate

The path is reduced to points at least 10 px apart, but `mouseDown` and
`mouseUp` events are always kept (`B39:2815-2818`, `B39:3333`). A click
position is therefore always available to the focus even when the pointer did
not move.

### 4.3 Take the samples inside one zoom

`gs` takes the decimated points inside `[sourceStartMs, sourceEndMs]`
(`B39:3334-3349`). It also tries to prepend the last point at or before the
start with a 1 ms offset; because the list is already range-filtered, this only
fires when a sample lands exactly on the start.

If the window contains no points at all, the fallback is:

- when mouse smoothing is enabled, the first pointer sample at or after the
  start within 500 ms;
- otherwise, the nearest recorded sample.

See `B39:3324-3332`.

### 4.4 Split into movement groups

The points are split greedily into groups. A group keeps accepting points while
its bounding box, including the candidate, fits inside a budget of **half the
visible source area at that zoom scale** (`B39:7916-7924`, grouping at
`B39:3159-3220`).

At scale 2, with a stage that matches the recording's aspect, that budget is
roughly a quarter of the crop in each axis. This is why the camera does not
chase every small move: one group can cover a large region.

### 4.5 Compute each group's centre

A group's target is its dwell-weighted centroid (`B39:2863-2878`):

```text
weight(point) = time until the next point in the group
                or the time the group ended (first point that did not fit)
                or 100 ms for a trailing point

centre = sum(position * weight) / sum(weight)
```

A pause therefore outweighs a fly-by. The centroid uses future points in the
same group, so the target is known as soon as the group starts.

### 4.6 Select the active group

At source time `t`, the camera uses the latest group whose first point is at or
before `t`. Before the first group it uses the first group (`B39:7929-7934`).

There is no interpolation between group centres. When the active group changes,
the target jumps and the spring performs the pan. This is how one zoom range can
contain several camera moves without being split into several items.

## 5. Framing the screen

The target group centre becomes a normalised point and a scale:

```text
position01 = centre / cropRect size
scale      = zoom item's zoom
```

See `B39:7936-7950`. Manual zooms use `manualTargetPoint` instead
(`B39:7955-7964`).

The screen resolver then applies three operations in `Rect.zoom`
(`YEi:1022-1033`, clamp at `YEi:998-1021`):

1. Move the resting screen rectangle so that `position01` lands on the zoom
   area's focal point. That is normally the centre of the zoom area, or the
   centre of the area left by the camera overlay in camera layouts.
2. Scale the rectangle by the zoom factor around `position01`.
3. Clamp the result inside the zoom area. If the scaled rectangle would leave
   the footage, it is shifted back; if it is smaller than the area, it is
   centred.

The clamp is why the pointer drifts off-centre near the edges of a recording.
It also prevents background gaps from appearing while zoomed.

One extra touch: the background stage is scaled by
`1 + (zoom - 1) * 0.125`, so a 2x zoom enlarges the stage by 1.125x. That
produces the subtle parallax between the screen and its backdrop
(`B39:7535-7540`).

## 6. Motion

### 6.1 A spring, not an easing curve

The screen rectangle is emitted as a spring target:

```text
rect: spring(rects.screen, screen.movementSpring)
```

See `B39:18148`, with the spring selected at `B39:7857-7862`.

The default `screenMovementSpring` is:

```text
stiffness 125
damping   12
mass      1.5
clamp     true
precision 0.002
```

See `CpX:53-58`. The integrator advances in 1 ms steps (`CpX:327-350`). The
clamp logic raises damping when the spring is moving too fast for its
displacement, which limits overshoot (`CpX:314-326`).

There is no explicit transition duration. The camera moves because the target
changes and the spring settles. This is the core of the feel.

### 6.2 Entry

By default the zoom target activates exactly at `sourceStartMs`, which is
already 300 ms before the click (`B39:1697-1710`). That pre-roll is the time
the spring has to get the camera into place before the interaction.

The project option `startAnimationEarly` adds another 350 ms of lead-in. It
defaults to false (`DgH:4385-4411`).

### 6.3 Exit

When the time leaves the range, the target becomes the resting frame again and
the same spring returns. The zoomed view is held for the whole range, which
typically ends 2.5 s after the click or release.

### 6.4 Instant zoom

Per item, `hasInstantAnimation` switches the screen spring to a zero spring,
and the instant window extends 50 ms past the range end (`B39:7908-7915`).
Generated ranges always set it to false (`DXX:1245`). A user can enable it per
zoom, and new zooms inherit it from the last-edited zoom style.

### 6.5 Panning inside one range

Because the active movement group can change during a range, one autozoom can
contain several camera moves. The range stays one item; the camera pans because
the group centre changes, not because the range was split.

`motionBlurAmount` defaults to 1 and the control ranges from 0 to 1
(`DgH:292-295`, `Brm6UpoL2:195-198`). The screen uses its current and previous
frame rectangles (`B39:9547-9549`, `B39:18015-18055`). The trail starts at the
current rectangle interpolated toward the previous rectangle by
`motionBlurAmount * fps / 60` (`B39:10094-10099`, `B39:12614-12632`); instant
zooms disable blur during their snap window (`B39:18139-18159`). The renderer
samples the current frame's texture along that trail, accumulating in squared
colour space and taking the square root on output (`B39:10440-10443`,
`B39:10478-10489`, `B39:10497-10541`). Export uses up to 16 texture fetches
at a 4-pixel span, while playback uses up to 6 at a 16-pixel span
(`B39:9688-9700`). Its GPU shader also uses texture gradients, dithering,
per-pixel coverage and rounded-edge reconstruction.

## 7. Manual zooms and guards

- The "Automatic zoom" button adds an automatic zoom of up to 3 s at the
  playhead. If the playhead is inside an existing zoom, it splits that zoom
  instead. See `BbP:72-116`.
- New zooms inherit the last-edited zoom's scale, presentation, loupe options,
  and instant flag (`CK1:32-64`). The initial defaults are 2x and the screen
  presentation.
- Clicking the canvas adds a manual zoom with a fixed `manualTargetPoint`
  (`B39:1527-1545`).
- Autozoom is unavailable when the recording has no mouse events, is an
  external-device capture, or the project capability is off (`DXX:3144-3146`).
  If that happens after a zoom exists, `Zoom.update` converts it to `manual` so
  its appearance is preserved (`DXX:3271-3276`).
- A disabled zoom, or a 1x zoom with screen presentation, has no visual effect
  (`DXX:3268-3269`).
- A loupe presentation uses the same target as a circle centre, with a small
  sinusoidal drift (`B39:7973-8017`). The target model is identical; only the
  presentation differs.

## 8. Behavioural consequences

- The range name says "click groups", but the framing follows **movement**
  groups. Clicks choose the windows; movement chooses the camera.
- Two clicks up to 5.3 s apart become one shot, not two.
- The camera is already moving 300 ms before the click. With
  `startAnimationEarly` it starts 650 ms before the click.
- A long press can extend the tail, because the release event seeds its own
  range.
- Trimming changes the framing path, because the path is restricted to footage
  still present in playback.
- A single range can pan several times. That is not a merge failure.
- The edge clamp, not the centroid, decides the shot when the focus is near the
  edge of the recording.

## 9. ApexShot today

ApexShot already has the same range constants and most of the camera model:

- `src/recording/editor/auto_zoom.rs:17-35` matches the lead, tail, merge gap,
  end ignore, end margin, start floor, and 2x scale.
- `windows-ui/src/video_editor/zoom.rs` already has movement groups with the
  half-viewport budget (`:844-891`), dwell-weighted centres (`:860-891`), the
  active-group rule (`:893-905`), and the same spring defaults, 125/12/1.5
  (`:16-18`).
- The 350 ms early-start option is also present (`:75-78`).

Resolved on `feat/autozoom-parity`:

- **Exit timing.** A zoom now holds its framing through `clip.end`, then the
  screen spring returns to the full frame. The 0.6 s early exit was removed from
  the camera plan and the static fallback in
  `windows-ui/src/video_editor/zoom.rs`. A later export check found the
  remaining post-clip shortcut: `evaluate` returned the static full frame as
  soon as the last clip ended, so the export cut instead of springing. That
  shortcut is gone; the camera now advances through the exit like any other
  target.
- **Click source.** The generator now adds a click seed at every press release
  as well as the recorded down event, in `clicks_from_sidecar`. The studied
  collection uses the whole `mouseclicks` stream (`DXX` line 1253), and that
  stream exposes both `mouseDown` and `mouseUp` (`YEiLxxgG` 2276-2292). The
  release time is exact; its position comes from the nearest pointer sample,
  falling back to the down click because the sidecar stores the press interval
  but not a separate release point.
- **Focus fallback.** The live camera already uses movement groups and their
  dwell-weighted centres. The stored centre is only the fallback when a range
  has no movement group. That fallback now follows the studied rule: the first
  pointer sample at or after the range start within 500 ms, otherwise the
  nearest sample (`B39:3324-3332`), and only then the click position.
- **Live focus fallback.** When an automatic clip's footage window has no
  pointer sample and none before it, `follow_points` now uses the same studied
  fallback (`B39:3334-3349`): the first sample at or after the window start
  within 500 ms, otherwise the nearest sample. The camera follows that sample
  instead of freezing on the stored centre.
- **Spring integration.** The Windows camera now runs the studied integrator
  exactly, once per composition frame, in place of the earlier 1/60 s grid
  (`CpX:314-392`, per-path advance `CpX:594-624`):
  - the overspeed damping clamp is chosen once per advance, not per step;
  - an advance of `d` ms runs `ceil(d)` steps, and the last step lasts
    `(ceil(d) - d)` ms, as the studied loop computes it. A 60 fps frame
    therefore integrates 16.33 ms and a 30 fps frame 33.67 ms, so the studied
    camera's speed depends slightly on the frame rate, and ours now does too;
  - the rest tolerance is set when a target changes, to
    `max(1, |value - target|) * 0.002`, or 1 when the value already equals the
    target. The earlier 0.01 threshold was not the studied rule.
  - The settle-time snap for very long advances (`CpX:353-382`) is not ported.
    The camera advances one frame at a time, so a frame never reaches it.
  - Each frame takes its target at the frame's own time, and an instant zoom
    snaps to its target through the window that ends 50 ms after the clip.
- **Pointer shake filter and click-keeping decimation.** The camera now reads
  pointer moves, presses, and releases (release positions from the nearest
  pointer sample), after the studied default shake filter: an event is dropped
  when both of the next two events are more than 500 px away in Manhattan
  distance, and a press dropped that way drops its release (`B39:2846-2862`,
  default `DgH:3254`, applied at `B39:3274-3284`). Decimation then keeps every
  press and release and drops moves within 10 px of the last kept event
  (`B39:2804-2826`). The filter runs in encoded-video pixels, while the studied
  filter runs in recording pixels after the crop, so the two differ only when
  the recording was scaled before encoding. The studied filter also runs before
  trimming, and ours runs per clip after it, which only matters right at a
  window edge.
- **Minimum window.** The generator no longer drops a window shorter than
  100 ms. After merging, `minimum_window` grows the end to the minimum, or
  shifts the start back at the end of the recording, matching the collection's
  `add` (`DXX:2667-2679`). With the current lead and tail constants a generated
  window is always at least about 199 ms, so this is a structural guard rather
  than a visible change.

- **Screen motion blur.** The Windows preview and exporter now use the current
  and previous camera rectangles, frame-rate-scaled strength, the studied
  playback/export fetch budgets, and squared-colour averaging. The amount is
  saved with the project and defaults to 1 for older files. Each fetch now
  covers its own step along the trail, the way the studied shader's gradient
  sample does: the renderer builds a box-filtered mip chain and takes up to
  four trilinear taps across the step at the matching level, mirroring the
  studied sampler's linear mip filter and 4x anisotropy (`vPAFtiYS:3859-3863`,
  `B39:10552-10575`). Fetches are jittered by the studied screen-space 8x8
  Bayer threshold (`vPAFtiYS:7333-7370`, `B39:12405-12408`), and both preview
  and export clamp at source edges like the studied clamp-to-edge sampler.
  Playback in the preview uses one tap per fetch to stay within frame budget.
  Cursor and webcam motion blur remain separate gaps. Preview effect work runs
  on a bounded worker, so slow frames can be skipped rather than blocking UI.
- **Rounded-edge reconstruction does not apply.** The studied shader
  reconstructs coverage of the card's rounded body, outline, and shadow along
  the trail (`B39:12259-12340`) because its zoom moves and grows the whole
  card across the stage. The Windows editor zooms by cropping the source inside
  a fixed card, so there is no moving card edge to smear. Matching that would
  mean adopting the studied framing model, which is a separate gap.
- **Zooms without movement groups now ride the spring.** Manual zooms, and
  automatic zooms with no pointer data, used the old static fallback: a
  smoothstep entry and a hard cut back to the full frame at `clip.end`. The
  studied build drives every non-instant zoom through the same screen spring,
  with only the target point differing (`B39:7955-7964`, `B39:18148`). The
  camera plan now springs these zooms in and out too.

- **No hold across gaps.** The camera used to hold an automatic zoom's
  framing across a gap of up to 0.5 s before the next one. The studied camera
  has no such hold: between zooms it targets the full frame, and the only
  look-ahead is the 350 ms early start when that option is on, which picks the
  next zoom starting at or after the current time and never an instant one
  (`B39:1697-1707`, `B39:8296-8303`, `DgH:4405-4411`, `YEi` `lt`/`ct`). The
  hold is removed.
- **Adding a zoom at the playhead.** An empty-lane click and the armed tool's
  Automatic zoom button now follow the studied add action (`BbP:72-127`, `DXX:2631-2643`): a new
  zoom runs 3 s or up to the next zoom, is refused under 100 ms, and splits the
  zoom under the playhead when the playhead is strictly inside one, selecting
  the right half. A new zoom starts at 2x, or at the scale and instant flag of
  the zoom last edited in the panel (`CK1:32-64`), and is automatic when the
  recording has pointer samples, manual otherwise (`DXX:3138-3146`). The
  last-edited style lasts for the editor session. The Hide track is unchanged.
- **The Add zoom tool.** The Zoom button arms a tool, as the studied
  `$toggleAddZoomTool` does (`BbP:128-145`): arming pauses and clears the
  selection, and the preview shows the whole frame with an outline the size
  of the zoomed view (the last-edited scale, 2x by default) following the
  pointer (`B39:1477-1527`). A click adds a manual zoom at the playhead aimed
  at the clicked point, through the same add rules, so it splits a zoom under
  the playhead into a manual right half (`B39:1528-1536`, `BbP:44-58`). When
  the recording has pointer samples the prompt also offers Automatic zoom,
  which adds the ordinary playhead zoom (`B39:1541-1545`). Adding selects the
  new zoom, and selecting any clip disarms the tool, as the studied
  `setSidebarItem` does (`B0e:2358-2362`); Escape and the Zoom button disarm it
  too (`B39:1565-1570`, `B0e:2329-2335`). Playing hides the prompt without
  disarming. One difference: the outline is clamped to the frame the way the
  placed zoom will frame it; the studied outline is centred on the pointer.
- **Not yet matched in the editor.** The studied build turns an automatic
  zoom manual when the recording has no mouse events (`DXX:3271-3276`); the
  Windows editor leaves it automatic, which frames the same way because it
  falls back to the stored centre.
- **Zoom levels.** The panel chips are 1.25x, 1.5x, 2x, 2.2x, 3.5x and 5x.
  The studied build offers 1x to 2.6x in 0.2 steps plus the default, on a
  1x-4x slider in 0.1 steps (`BbP:274-338`).

There is no remaining confirmed generator gap.

## 10. Limits

- This is static analysis of the deobfuscated 4.1.0-5005 renderer. The app was
  not run and no rendered frames were measured.
- The main process is V8 bytecode, so import-time creation of zoom ranges is not
  proven. The renderer proves the generator and the "Restore default zoom
  ranges" path.
- The contents of `mouseclicks` are read from the getters that split it into
  down and up events. No recorded project fixture was inspected.
- The spring behaviour is read from the engine source. Perceived motion also
  depends on project settings, frame rate, and motion blur, which were not
  measured.

## 11. Resolved questions

1. **Movement-group budget.** The Windows editor's camera frame is the source
   rectangle, so `frame / scale` is the visible source at the current scale and
   `frame / scale * 0.5` is the studied half-viewport budget. The studied
   stage's padding and letterboxing sit outside that source rectangle.
2. **Range generation entry point.** The renderer exposes the generator through
   `resetZoomRanges`, which the "Restore default zoom ranges" action calls
   (`DXX:3661-3662`, `DSYc1gpA:208-214`). No import-time caller is visible.
   ApexShot's Detect action is the equivalent entry point.

## 12. Validation

Exit-timing change, 2026-11-09:

- `cargo check --bin apexshot-video-editor` succeeded.
- `cargo test --lib video_editor::zoom::tests` passed 23 of 23 tests.
- `cargo test --lib` passed 822 tests with 12 ignored. One unrelated
  `settings::usage` layout test failed in the parallel run and passed when run
  alone.
- `cargo clippy --all-targets` finished without warnings.
- Physical harness: `artifacts/physical/zoom_exit_harness.py` opens a Tk window
  that records its own clock, click phase, hold, and release, then records that
  window through the capture test's `APEXSHOT_TEST_WINDOW_TITLE` path. The clip
  is `artifacts/physical/zoom-exit-ui.mp4`; the sidecar has one click at recording
  `5.020 s`, region `(901, 508)`, which is the crosshair.
- The clip begins with a 3.5 s pre-roll. Its `T+` clock then runs:

  | UI time | Expected editor |
  | --- | --- |
  | `0.000-1.500` | full frame, 1.00x |
  | `1.700-2.136` | lead-in; camera may start moving |
  | `2.136` | click at the crosshair |
  | `2.236-4.500` | 2.00x, centered, held |
  | `3.900` | red tick; the old exit would have started returning |
  | `4.500-5.500` | spring back to 1.00x |
  | `5.500-7.500` | full frame |

  The sidecar click maps to an automatic-zoom range of `4.720-7.520 s` in the
  recording. The old timing would have begun returning at `6.920 s`, which is
  about `T+4.04`; the new build must still be at 2.00x at `T+4.1` and only
  start returning after `T+4.5`.
- The editor was opened with the clip and left running; the maintainer presses
  Detect and checks the hold. Physical confirmation is still pending.

Click-source change, 2026-11-09:

- `cargo fmt --all -- --check` exited 0.
- `cargo clippy --all-targets` finished without warnings.
- `cargo test --lib auto_zoom` passed 18 of 18 tests.
- `cargo test --lib` passed 826 tests with 12 ignored. The known parallel-only
  `settings::usage` layout failure occurred once and passed when run alone.
- The editor binary rebuilt with the change.
- Physical clip: `artifacts/physical/zoom-click-source.mp4`, recorded through
  `zoom_click_source_harness.py`. The sidecar has one click at `4.985 s` and one
  press from `4.985 s` to `6.482 s`, at region `(901, 508)`, the crosshair.
- Old range: `4.685-7.485 s`. Release-extended range: `4.685-8.982 s`, because
  the `6.482 s` release adds a window and the two merge.
- The clip's `T+` clock puts the press at `2.012 s`, the release at `3.552 s`,
  the old end at about `4.512 s`, and the new end at about `6.009 s`. At
  `T+4.7` the editor must still be at 2.00x; the old build would already be back
  at the full frame. The editor was opened with this clip and left running for
  the maintainer's Detect check.

Focus-fallback change, 2026-11-09:

- `cargo test --lib auto_zoom` passed 19 of 19 tests, including the new
  nearest-pointer fallback and the existing group-centre tests.
- `cargo test --lib` passed 827 tests with 12 ignored. The known parallel-only
  `settings::usage` layout failure occurred once and passed alone.
- `rustfmt --check` on the changed file passed. The workspace-wide format check
  still reports the checkout's CRLF newline style on untouched files.
- `cargo clippy --all-targets` finished without warnings.
- No new physical clip: the existing recordings have pointer samples, so they
  exercise the movement-group path, which is unchanged. The fallback is covered
  directly by the new unit test.

Minimum-window change, 2026-11-09:

- `cargo test --lib auto_zoom` passed 21 of 21 tests, including the two new
  minimum-window cases.
- `cargo test --lib` passed 829 tests with 12 ignored. The known parallel-only
  `settings::usage` layout test failed and passed when run alone.
- `rustfmt --check` on the changed file passed.
- `cargo clippy --all-targets` finished without warnings.
- The editor binary rebuilt with the change.
- No physical clip: the current lead and tail constants make every generated
  window at least about 199 ms, so the expansion cannot be reached from a click
  recording. The helper is covered directly instead.

Live-follow fallback, 2026-11-09:

- `cargo test --lib video_editor::zoom::tests` passed 25 of 25 tests.
- `cargo test --lib` passed 831 tests with 12 ignored. The known parallel-only
  `settings::usage` layout test failed and passed when run alone.
- `rustfmt --check` on the changed file passed.
- `cargo clippy --all-targets` finished without warnings.
- The editor binary rebuilt with the change.
- No physical clip: the existing recordings have pointer samples inside their
  zoom windows. A clip window with no sample and none before it is covered by
  the new camera unit test and the lookahead-selection test.

Spring-integration change, 2026-11-09:

- `cargo test --lib video_editor::zoom::tests` passed 27 of 27 tests, including
  the 1 ms integration and rest-threshold cases.
- `cargo test --lib` passed 833 tests with 12 ignored. The known parallel-only
  `settings::usage` layout test failed and passed when run alone.
- `rustfmt --check` on the changed file passed.
- `cargo clippy --all-targets` finished without warnings.
- The editor binary rebuilt with the change.
- No new physical clip: the change is a sub-frame integration correction, and
  the existing spring-motion tests cover the trajectory and rest behavior.

Export regression, 2026-11-09:

- Exported `artifacts/physical/zoom-click-source.mp4` through the real exporter
  with the saved project state: range `4.685-8.982 s`, 2x, centre `(901,
  507.3945)`, 1202x838, 60 fps, 883 frames.
- The export exposed a real exit bug: at `8.9 s` the frame was still 2x, but at
  `9.1 s` it was already full frame. The camera plan was returning the static
  frame as soon as the last clip ended, bypassing the spring exit.
- The plan now keeps advancing after the last clip. Re-exported frames show the
  spring return: 2x at `8.9 s`, still cropped at `9.1 s`, wider at `9.3 s`, and
  full by `9.6-10.0 s`.
- `cargo test --lib video_editor::zoom::tests` passed 28 of 28 tests, including
  the new post-clip spring-exit test.
- The temporary fixture export test was removed before committing; the shipped
  coverage is the camera exit test.

Screen motion blur, 2026-11-09:

- The click-source fixture exported at 1202x838, 60 fps, 883 frames in 23.69 s
  with the debug exporter. The motion-blurred export differs from the prior
  unblurred export most during the entry and exit, and much less during the
  held section. Encoding differences mean this is not an exact blur metric.
- A portable FFmpeg integration test exported with blur on and off at 24 and
  60 fps, and checked moving frames differ more than resting frames. Run it with
  `APEXSHOT_EXPORT_TEST=1` and
  `cargo test --lib video_editor::export::tests::motion_blur_reaches_real_export_frames`.
- A debug 1280x720 high-motion preview sample took about 31 ms per frame before
  moving the effect to a bounded worker. This is not an end-to-end playback
  frame-rate measurement. No rendered studied-build fixture was available, so
  visual and numerical parity remain unverified.

Screen motion-blur sampling and manual-zoom spring, 2026-11-10:

- Real exports of `artifacts/physical/zoom-click-source.mp4` (0-11 s, a 2x
  zoom over 4.685-8.982 s) at 24, 30, and 60 fps with blur on and off, plus a
  pan into the top-left corner, an instant zoom, and a cut variant. Exports
  showed the hard cut at the end of zooms without pointer data: full frame
  18 ms after `clip.end`. After the spring change, blur-on and blur-off frames
  differ through the entry (4.70-5.10 s) and the exit (9.00-9.90 s), and are
  near identical at rest and through the hold. The instant zoom still snaps.
- Accuracy against a brute-force reference: one source frame rendered with up
  to 256 point samples per pixel at a quarter-texel span stands in for the
  continuous trail. Mean absolute error, 0-255 per channel:

  | Case | No blur | Export (16 fetches, 4 taps) | Playback (6 fetches, 1 tap) |
  | --- | --- | --- | --- |
  | Zoom entry, 1.45x to 1.6x | 11.0 | 0.77 (p99 7) | 3.97 (p99 36) |
  | Pan, 2x, 60 px by 20 px | 14.7 | 1.09 (p99 8) | 4.21 (p99 32) |

  This checks that the renderer integrates the trail. It is not studied-build
  evidence: the studied build ships only as a macOS app, so no frames it
  rendered were available, and visual and numerical parity with it remain
  unverified.
- `cargo test --lib video_editor` passed 305 with 5 ignored, and
  `cargo clippy --all-targets` finished without warnings. Three camera tests
  that asserted the old static fallback now assert the spring entry, hold,
  and exit instead.

Studied spring and pointer filters, 2026-11-10:

- Evidence from the studied code itself: `artifacts/studied-harness/` (local,
  untracked) copies the studied spring (`CpX:283-392`), shake filter, and
  decimation (`B39:2801-2862`) verbatim and runs them under Node. The camera
  test `the_camera_spring_matches_the_studied_integrator_frame_by_frame`
  checks our camera against those outputs within 1e-6: a 600 px move from rest
  at 60, 30, and 24 fps, and an overspeed start that triggers the damping
  clamp. The pointer-path tests reproduce the harness's shake and decimation
  outputs event for event. One studied behaviour is easy to misread: a press
  far from the moves around it is kept, because its own release is next to it.
  The release and the move before the press are dropped instead.
- `the_follow_camera_ignores_a_lone_pointer_spike` fails with the filter
  bypassed: the camera is pulled 6.5 px toward the spike at 2.05 s.
- `cargo test --lib video_editor` passed 314 with 5 ignored,
  `cargo test --lib auto_zoom` passed 22, `rustfmt --check` on the changed
  files passed, and `cargo clippy --all-targets` finished without warnings.
