# Auto zoom refinement

Replacing the multi-signal automatic zoom detector with a small, predictable
model: **automatic zooms come from recorded clicks only.**

## Target behaviour

| Rule | Value |
| --- | --- |
| Window opens before the click | 0.3 s |
| Window runs after the click | 2.5 s |
| Clicks this close share one zoom | 2.5 s |
| Clicks in the last | 1.0 s are ignored |
| A zoom never reaches the last | 0.8 s of the recording |
| Default zoom level | 2.0× |
| Shortest window kept | 0.1 s |

Clicks seed the windows. Pointer movement over a window decides where it looks
(the middle of the range the pointer covered). Windows are merged while they
are no further apart than the merge gap, so a burst of clicks becomes one shot
and overlapping zooms cannot be produced. Each result is a source-time window
with a pixel focus, so trimming, cutting, or retiming keeps the zoom on its
footage.

A Detect pass acts on what it finds: the zooms land on the timeline directly,
as one undo step. There is no separate review step.

## Why this replaces the current detector

The current detector blends click sessions, purposeful landings, spatial
splits, priority scoring, and a region fit. Each rule needs its own constants
and its own failure mode, and the interactions between them already produced
overlapping candidates and a staged count that `Apply` could not honour. The
click-window model removes the class of bug instead of patching it: windows
merge before they are ever placed.

The camera that follows inside a zoom now chases the movement-group centre
on a spring (phase 3), replacing the earlier edge-feathering follow.

## Phases

1. **Window generator** — `auto_zoom` module: click windows, merge, clamp,
   focus, source anchoring. Done.
2. **Wire in and remove the old detector** — placement uses the generator;
   the old multi-signal detector and the clicks/hovers choice are gone. Done.
3. **Camera follow** — the follow camera chases the movement-group centre
   active at the current time on a damped spring, stiffening near a click
   and snapping per zoom when instant is set. Portable: group-centre target,
   click-proximity stiffness, per-zoom instant. Omitted: drag/release
   stiffness and typing suppression (mouse press intervals are now recorded,
   but no drag spring is wired until its value is agreed; key identities are
   never collected). Done.
4. **Style inheritance** — a new zoom opens with the level and style of the
   last zoom the user edited. Done.
5. **Instant toggle** — each automatic zoom carries an instant-vs-animated
   flag. Animated is the follow camera; instant snaps with no eased scale
   ramp, no morph from a neighbour, and no follow recenter. Done.

## Motion decisions

The studied behaviour drives zoom motion with springs plus a per-zoom
instant flag — there are no named easing presets. The port follows that
split where the camera exists and keeps our own controls where it does
not:

- Automatic clips show an Instant switch instead of easing presets.
  Animated chases the movement-group centre on the follow spring;
  instant jumps to it. The flag is stored per zoom,
  inherited by newly added zooms, and always opens off for generated
  zooms. Older projects load it as animated.
- Manual clips keep the Glide/Smooth/Snappy/Linear presets. A manual zoom
  is a fixed focus point, not a follow camera, so the presets remain its
  motion control. This is a deliberate local divergence, not a studied
  behaviour.
- The Classic Animation switch is gone from the panel: no counterpart was
  found in the studied behaviour, where the follow is simply always on.
  The project-wide `zoom_classic` field is still honoured when an older
  project carries it (new projects default it off and the switch cannot
  turn it back on); Reset clears it, which is the migration path off.

## Loupe decision

Explicitly skipped. The studied presentation pairs a screen-vs-bubble
switch with per-zoom bubble options (radius, bevel, chromatic aberration,
glass optics) and a dedicated composite. That is a new renderer plus panel
plus translations for a look the current full-frame zoom already covers.
If that changes, it ports as its own phase: model, composite, minimal UI.

## Non-goals

- No hover-only or inferred-motion automatic zooms.
- No per-zoom region fit; the level is the model default (see phase 4 for
  inheriting a user's edited level).
