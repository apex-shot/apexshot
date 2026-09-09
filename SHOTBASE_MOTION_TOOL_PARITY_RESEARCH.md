# Shotbase Image-Motion Tool Parity Research

## Scope correction and confirmed conclusion

This report compares **only** Shotbase’s image-editor Motion mode with Apexshot’s capture-image Motion mode. It does not evaluate Apexshot’s recording/video editor, its video cursor tooling, its video background settings, or video-export audio behavior. Those surfaces are out of scope and must not be used to claim Motion parity.

The comparison is therefore:

```text
Shotbase captured image → Motion editor → animated export
                 versus
Apexshot captured image → Motion editor → animated MP4 export
```

Within that scope, the earlier high-level conclusion holds and is now more precise: Apexshot has implemented the Motion transform/timeline work and most of Shotbase’s **Appearance / Background** card-scene foundation. It has not implemented the remaining Shotbase image-Motion tools: Browser Chrome, Scene Shadows, Frame presets, Motion cursor overlay, Motion camera overlay, or image-Motion audio tracks. Watermark now has a user-image layer with size, inset, and XY controls; it does not yet replicate a Shotbase built-in catalog or byte-backed project resource. Border and the recovered Shadow controls are implemented.

## Evidence and method

| Item | Verified evidence | Confidence |
| --- | --- | --- |
| Shotbase artifact | `~/Desktop/Research/Shotbase.dmg`; SHA-256 `b0b8f64f0cf89fc5f53df588eb22ec77d20e3b048b38f39265557bdb7a474ba9`. | High |
| App build | Expanded `Shotbase.app` is version `1.4.0`, build `13`; the executable is a universal Mach-O. | High |
| Executable | `Shotbase.app/Contents/MacOS/Shotbase`; SHA-256 `f4e0fc21cb6666a78a427a5b0d6567a4374a4feaea5c77a0d2b993f6e7a384f5`. | High |
| Static method | Printable-string extraction from the binary and `Assets.car`; x86_64 Mach-O disassembly of the compiled Motion inspector and bound-control callbacks; plus direct inspection of Apexshot’s capture-image Motion UI and renderer. | High for named model/UI/assets; medium-to-high for the traced control wiring |
| Limitation | The macOS app could not be run from this Linux workspace. The disassembly proves selected compiled control construction and value-binding mechanics, but it does not recover Swift source names, every control’s range/default, pixel algorithms, or all runtime-only interaction details. | High |

The Shotbase binary’s image-capture record contains `imageData`, `isImageDataCanonicalFlattenedSource`, `editorMode`, `motionDuration`, `flattenedStaticSourceVersion`, `staticEditingSourceVersion`, and `motionCacheDuration`, followed directly by the Motion scene/tool fields below. This is the relevant image-editor evidence, not an inference from a video editor.

The original research phase changed no application implementation. The follow-up work recorded here adds the first user-image Watermark layer.

## Recovered Shotbase image-Motion structure

Shotbase’s image Motion editor contains the peer sections `orientation`, `appearance`, `cursor`, `overlays`, `frame`, `motion…`, `camera`, `audio`, and `textEffects`; the mode pair is `Static` / `Motion`. Motion is consequently a layered image-to-animation compositor, not just a transform timeline.

The persisted state following the image Motion fields contains:

```text
Appearance
  backgroundPadding, backgroundFillType, backgroundColor,
  gradientColor1, gradientColor2, selectedGradientPresetIndex,
  wallpaperImageName, customBackgroundImage, backgroundBlur,
  backgroundNoise, borderRadius, borderThickness, borderFillColor,
  shadowBlur, shadowOpacity, shadowPositionX/Y,
  browserEffect, browserURL, browserTabText, browserScale

Overlays and frame
  watermarkActiveId, watermarkSize, watermarkInset, watermarkPositionX/Y,
  watermarkImageData, watermarkFileName,
  sceneShadowPresetId, sceneShadowOpacity, sceneShadowPlacement,
  framePresetId

Cursor
  cursorShow, cursorSize, cursorRotation, cursorAlwaysPointer, cursorSkin,
  cursorSmoothingEnabled, cursorSmoothingTension, cursorSmoothingFriction,
  cursorSmoothingMass, cursorTiltAmount, cursorTrackFile

Camera and audio
  cameraShow, cameraMirror, cameraTrackFile, cameraShape, cameraSize,
  cameraRoundness, cameraPositionX/Y, cameraShrinkDuringZoom,
  cameraShrinkSizeMultiplier,
  microphoneTrackFile, systemAudioTrackFile,
  isMicrophoneMuted, isSystemAudioMuted
```

Shotbase’s Motion render input separately names `appearanceSnapshot`, `watermarkSnapshot`, `sceneShadowSnapshot`, `frameSnapshot`, `cursorSnapshot`, and `cameraSnapshot`. It also names `sourceImageOverride`, which confirms this render architecture is applicable to the image-Motion path. The named render layers include `background`, `shadow`, `border`, `browserChrome`, `watermark`, `sceneShadowOverlay`, and `sceneShadowUnderlay`.

## Disassembly-confirmed control wiring

This section is deliberately narrower than a source-code reconstruction: it reports only behavior visible in the compiled x86_64 instruction stream. The inspected x86_64 image has VM base `0x100000000`; all addresses below are executable addresses in that image, not guesses from string offsets.

| Trace | Direct machine-code observation | What it establishes |
| --- | --- | --- |
| Inspector builder `0x100d1d840` | A compiled builder allocates and assembles the inspector configuration. At `0x100d1dd4f` it writes the compact Swift string `Background` into a configuration structure and stores callback `0x100d23e50`. | Background is an instantiated inspector section, not a dead localized string. |
| Background callback | `0x100d23e50` passes its captured context and tail-calls `0x100d1e3f0`, a separate, substantial section-builder body. | Background has distinct executable content and captured state/context. |
| Browser section | The same parent builder writes `Browser` at `0x100d1e174` and stores callback `0x100d23e80`; that callback tail-calls the independent builder at `0x100d21ce0`. | Browser is a separate functional Motion inspector/compositor feature family, not a Border label or a cosmetic alias. |
| Bound range control | Inside the Browser builder, a range-like control is constructed with an explicit upper literal `166.0` at `0x100d2237a`; the bound getter/setter closures are installed at `0x100d23f40` / `0x100d23f60`. | At least one Browser property is a real editable numeric control with compiled bounds and two-way binding. The exact user-facing label is not asserted solely from this trace. |
| Normalized value mutation | The setter reached through `0x100d23f60` enters `0x100d23060`: it reads the input `Double`, divides it by the binary constant `100.0` at `0x10209ffa8`, compares it with the captured reference’s `Double` at offset `+0xf8`, writes changed values, then invokes update/observation work. Its paired getter beginning at `0x100d22fd0` multiplies the stored value by the same `100.0`. | This is direct evidence of a live two-way control whose UI-scale number is normalized by a factor of 100 in bound storage, rather than a display-only slider. It does **not** by itself identify that `+0xf8` member’s Swift field name or prove disk-write timing. |
| Additional Appearance controls | The same compiled inspector flow embeds the labels `Blur`, `Noise`, `Padding`, and `Position` and creates their control configuration/callback data. | These Appearance controls are executable UI construction, corroborating the separately recovered persisted fields. |

The strongest implementation-level conclusion is therefore architectural: Shotbase creates independent inspector sections, each with captured bindings, and passes a snapshot per tool family into a renderer that owns named compositor layers. A faithful implementation should preserve that separation; it should not flatten all missing tools into the existing `MotionAppearance` struct or treat them as variants of the card border/shadow.

## Image-Motion parity matrix

Status meanings: **Present** = Apexshot’s capture-image Motion editor contains the corresponding UI/state/rendering path; **Partial** = some but not all Shotbase behavior is implemented there; **Absent** = no equivalent was found in Apexshot’s capture-image Motion UI/state/renderer. No recording/video-editor feature affects these statuses.

| Shotbase image-Motion tool | Verified Shotbase behavior | Apexshot image-Motion equivalent | Status |
| --- | --- | --- | --- |
| Appearance → Background | Fill cases `none`, `color`, `gradient`, `wallpaper`, `image`; padding, blur, noise; gradient-preset index; wallpaper/custom-image state. | `MotionBackgroundFillType` has the same five fill cases. The Appearance inspector provides fill selection, color and two-stop gradient editing, selected wallpaper/image files, padding, background blur, noise, preview, and MP4 export rendering. | Partial-to-strong |
| Appearance → Border | Border radius, thickness, and fill color; explicit `Border` UI. | Motion UI and renderer implement color, thickness, and radius on the image card. | Present |
| Appearance → Shadow | Shadow blur, opacity, and position; explicit `Shadow`, `Blur`, `Opacity`, and `Position` UI strings. | Motion state, renderer, and Appearance inspector expose blur, opacity, and X/Y position. | Present |
| Appearance → Browser | Browser effect, URL, tab text, and scale; explicit `Browser` section. | No browser-chrome image-Motion state, inspector, or compositor layer. | Absent |
| Overlays → Watermark | Image bytes/file name, active ID, size, inset, position; `Watermark`, `Size`, `Inset`, and `Position` UI strings. | Dedicated image-Motion watermark state, image chooser, size/inset/XY controls, and card-projected preview/MP4 compositor layer. User-owned file paths are retained rather than Shotbase-style bytes/catalog IDs. | Partial |
| Overlays → Scene Shadows | Preset ID, opacity, placement; overlay/underlay render-layer distinction; 14 numbered shipped shadow presets. | No image-Motion scene-shadow asset system, placement control, or compositing layer. | Absent |
| Frame | `framePresetId`; explicit `Frame` UI. Labels include Standard, Instagram, X (Twitter), and YouTube. | No frame-preset state, selector, or image-Motion renderer. | Absent |
| Cursor | Visibility, skin, size, rotation, always-pointer, spring smoothing parameters, tilt, and dedicated cursor track. | No cursor track/state, cursor inspector, or cursor compositor stage in capture-image Motion. | Absent |
| Camera | Visibility, mirror, separate track, shape, size, roundness, position, shrink-during-zoom, and shrink multiplier. | No camera overlay track/state/UI/compositor in capture-image Motion. | Absent |
| Audio | Separate microphone and system-audio tracks plus separate mute states. | No image-Motion audio source, source-track state, audio UI, or audio attachment to the still-image MP4 export. | Absent |

## Detailed findings

### Appearance → Background

Shotbase exposes five recoverable fill values: `none`, `color`, `gradient`, `wallpaper`, and `image`. Its assets include 55 embedded wallpapers (`wallpaper-001` through `wallpaper-055`) with matching thumbnail names. The recovered `selectedGradientPresetIndex` also proves that Shotbase’s gradient selection includes a preset concept in addition to individual colors, although strings alone do not establish the exact number or visuals of those presets.

Apexshot’s capture-image Motion mode matches the five fill types directly. `build_motion_appearance_panel` creates selectors for None, Color, Gradient, Wallpapers, and Image; Wallpapers now opens a compact strip of bundled app backgrounds with an expandable full grid, while Image retains a user-file chooser. It also adjusts padding, blur, and noise. `motion_render.rs` renders these effects for both the image-Motion preview and MP4 export.

The remaining confirmed Background gap is catalog parity. Apexshot Motion now reuses its bundled app-background catalog for Wallpapers, but it does not ship Shotbase’s recovered 55-wallpaper catalog or its gradient-preset model. Image remains a user-chosen file and Gradient remains a custom two-color fill.

### Appearance → Border and Shadow

Shotbase stores and labels Border separately from Shadow. The evidence supports:

- Border: radius, thickness, fill color.
- Shadow: blur, opacity, and position.

Apexshot’s image-Motion card renderer applies radius, border stroke, and sampled drop shadow using `shadow_blur`, `shadow_opacity`, and `shadow_position`. Its Appearance inspector offers Border color, Border thickness, Border Radius, and Shadow opacity, blur, and X/Y position. Border and Shadow are complete at the recovered-field level.

### Appearance → Browser Chrome

Shotbase has a standalone Browser section with stored `browserEffect`, `browserURL`, `browserTabText`, and `browserScale`. The render layer is separately named `browserChrome`, so it must not be treated as a Border variation.

No browser chrome/effect, tab text, URL, scale, or browser-compositor layer exists in Apexshot’s capture-image Motion source. This is an absent image-Motion feature family.

### Overlays → Watermark and Scene Shadows

Shotbase’s Watermark state holds source image data/file name, active ID, size, inset, and XY position. Scene Shadows are a different system: they store preset ID, opacity, and placement. The asset catalog has `Shadow-01` through `Shadow-14`, matching thumbnails, plus distinct `Shadow-Overlay` and `Shadow-Underlay` assets; render-layer strings preserve the above/below-content distinction.

Scene Shadows do not yet exist in Apexshot’s capture-image Motion compositor. Apexshot now has a distinct user-image Watermark layer, but not Shotbase’s byte-backed/catalog resource semantics. Its regular card drop shadow is not Scene Shadows: it has no image preset, no overlay/underlay placement, and no independent asset selection.

### Frame

Shotbase stores a `framePresetId` and exposes a `Frame` section. Recovered labels identify Standard, Instagram, X (Twitter), and YouTube presets. The strings do not prove their exact dimensions or safe-area rules, so those are intentionally not claimed.

No comparable image-Motion frame preset state, UI, or renderer is present in Apexshot.

### Cursor

Shotbase’s image-Motion cursor is an independent, tracked overlay. Recovered fields cover show/hide, size, rotation, force-pointer behavior, skin, spring smoothing enablement and tension/friction/mass, tilt, and a cursor track file. The accompanying UI strings include Show cursor, Style, Size, Rotate, Movement, Tilt, Snappiness, Drag, and Weight. Shipped cursor assets include WedgeCursor, CircleCursor, and a default cursor set.

None of those cursor-overlay components occurs in Apexshot’s capture-image Motion pipeline. This finding does **not** say that other Apexshot surfaces lack cursor work; it says that the image-Motion feature requested here has no cursor track or image-Motion rendering layer.

### Camera and Audio

Shotbase’s image-Motion record and render input include an independently composited camera: show, mirror, media track, shape, size, roundness, position, and shrink-during-zoom behavior. Shape UI strings enumerate Auto, Square, Wide, and Tall. It also includes separate microphone and system-audio tracks, each with its own mute state.

No camera overlay or audio-track pipeline was found in Apexshot’s capture-image Motion mode. The still-image Motion exporter creates an MP4 from a rendered image scene; it does not attach a camera, microphone, or system-audio source. These are absent for the scoped path.

## Accurate implementation boundary in Apexshot

The capture-image Motion feature is centered in these paths:

| Path | Verified responsibility |
| --- | --- |
| `src/capture/editor/window/motion_mode.rs` | Static/Motion mode UI, Motion timeline, Appearance inspector, background-file chooser, and Motion export entry point. |
| `src/capture/editor/window/motion_render.rs` | Image-card scene compositor used by preview and MP4 export; background, border, shadow, perspective/card transforms, and text rendering. |
| `src/recording/editor/model_parts/types.rs` | Shared `MotionState` / `MotionAppearance` data definition used by the capture-image Motion feature. This shared location is not evidence that the recording editor is in scope. |

The Appearance and Watermark data are genuine image-Motion foundations, not UI-only scaffolding: image-Motion preview/export both use them. The remaining non-Motion Shotbase sections listed as Absent need their own image-Motion state, inspector, resource handling, and compositor stages before they can be called parity features.

## Verified parity contract — without inventing missing behavior

The following is the smallest contract supported by Shotbase’s record/snapshot/layer evidence. It is useful for planning, but it intentionally leaves values blank where the binary did not establish them. In particular, it does **not** prescribe z-order beyond the explicit Scene Shadow overlay/underlay distinction.

| Tool family | Persisted input contract recovered from Shotbase | Render contract recovered from Shotbase | Not recovered; must not be guessed |
| --- | --- | --- | --- |
| Browser Chrome | effect, URL, tab text, scale | named `browserChrome` layer; independent Browser inspector callback | effect enum values, chrome geometry, URL parsing behavior, final z-order |
| Watermark | active ID, image bytes, file name, size, inset, X/Y position | own `watermarkSnapshot` and named `watermark` layer | built-in watermark catalog, coordinate origin, units, z-order |
| Scene Shadows | preset ID, opacity, placement | own `sceneShadowSnapshot`; distinct `sceneShadowOverlay` and `sceneShadowUnderlay` layers | preset-to-asset mapping, exact placement enum values, blend/opacity math |
| Frame | preset ID | own `frameSnapshot` | frame dimensions, masks, safe areas, position relative to other layers |
| Cursor | show, skin, size, rotation, always-pointer, smoothing flag/tension/friction/mass, tilt, track file | own `cursorSnapshot` and cursor track input | track-file schema/timestamps, smoothing equations, interpolation, final layer position |
| Camera | show, mirror, track file, shape, size, roundness, X/Y position, shrink-on-zoom, shrink multiplier | own `cameraSnapshot` and camera track URL | media format, shape geometry, zoom coupling equation, final z-order |
| Audio | microphone track file, system-audio track file, two mute flags | microphone and system-audio track URLs supplied to Motion render/export input | mixing, gain, timing, codec/container choices |

This is why the next work should begin with independent image-Motion snapshots/state and preview/export composition stages. The Shotbase evidence supports those boundaries directly. Choosing algorithms, coordinate systems, default values, or layer order without additional runtime evidence would be a new Apexshot design decision, not replication.

## Recommended remaining image-Motion parity order

1. Expand the bundled background/gradient preset library if closer catalog parity is needed; the Motion Wallpapers picker now reuses the app’s existing catalog.
2. Extend the compositing-layer contract for the remaining image-Motion tools: durable state, preview/export equivalence, resource ownership, and explicit z-order. The Shotbase snapshot names establish that Browser Chrome, Watermark, Scene Shadows, Frame, Cursor, and Camera are independent layers.
3. Add Frame as a deterministic image layer. Watermark’s first user-image layer is implemented; catalog/resource ownership parity remains optional follow-up work.
4. Add Scene Shadows as asset-backed overlay/underlay placement—not as an extension of card drop shadow.
5. Add the tracked Cursor and Camera layers, including their inputs and interaction with the existing Motion transform/zoom path.
6. Add separate image-Motion microphone/system-audio track handling and mixing only after the visual layer contract is stable.
7. Add Browser Chrome as its own final layer; its URL/tab/effect/scale fields make it distinct from Background or Border.

## Guardrails and non-findings

- No video-editor result is used in this report.
- No exact Shotbase slider ranges, defaults, overlay order, or full frame-preset dimensions are asserted; static strings cannot prove them.
- The presence of camera/audio fields in the Shotbase image-Motion record proves those tool families ship with this path, but not the exact workflow by which a user supplies their media.
- Existing workspace changes were preserved. The only file created/updated for this task is this Markdown report.

## Sources

1. Private artifact: `~/Desktop/Research/Shotbase.dmg` and its expanded `Shotbase.app`.
2. Private artifact: `Shotbase.app/Contents/Info.plist` — application version and media/document declarations.
3. Private artifact: `Shotbase.app/Contents/MacOS/Shotbase` — image-Motion model fields, UI labels, render snapshots, and compositor-layer names.
4. Private artifact: `Shotbase.app/Contents/Resources/Assets.car` — wallpaper, cursor, and scene-shadow asset names.
5. Apexshot: `src/capture/editor/window/motion_mode.rs` and `src/capture/editor/window/motion_render.rs` — scoped image-Motion implementation.
6. Apexshot: `src/recording/editor/model_parts/types.rs` — shared Motion state definition consumed by the scoped image-Motion implementation.
