# Large File Refactoring Plan

## Goal

Split the largest source files into focused files, generally targeting 300-800 lines per file, without deleting or changing functionality.

The refactoring must preserve:

- Existing behavior and public module paths
- Rendering and CSS ordering
- GTK callback and main-thread behavior
- C++ drawing and hit-test geometry
- Process, descriptor, session, and resource lifetime ordering
- Existing test coverage

Every initial extraction should be a mechanical move. Cleanup, deduplication, API redesign, and behavior changes should happen separately after parity has been established.

## Recommended Order

- [x] Split the large editor test file.
- [x] Split recording editor model types using the existing `include!` architecture.
- [x] Split editor CSS while retaining one provider and exact source order.
- [x] Split settings CSS while retaining one provider and exact source order.
- [x] Split pure Motion rendering helpers.
- [x] Split `capture_overlay.rs` behind a compatibility facade.
- [x] Split Motion UI and controller code.
- [x] Split C++ drawing code.
- [x] Split the recording backend last because it has the greatest lifecycle risk.
- [x] Extract Motion coordination from the editor window root after its dependencies are smaller.

## 1. Editor Tests (Complete)

Current file: `src/capture/editor/tests.rs` (about 2,022 lines).

Keep `tests.rs` as an aggregator so the tests remain inside `capture::editor` and retain access to private and `#[cfg(test)]` APIs.

```text
src/capture/editor/tests.rs
src/capture/editor/tests/tools.rs
src/capture/editor/tests/cursor.rs
src/capture/editor/tests/colors.rs
src/capture/editor/tests/tool_style.rs
src/capture/editor/tests/history.rs
src/capture/editor/tests/selection.rs
src/capture/editor/tests/text.rs
src/capture/editor/tests/numbering.rs
src/capture/editor/tests/effects.rs
src/capture/editor/tests/export.rs
src/capture/editor/tests/crop.rs
src/capture/editor/tests/drag_draw.rs
src/capture/editor/tests/transform.rs
src/capture/editor/tests/arrows.rs
```

Requirements:

- Move all 87 tests verbatim initially.
- Preserve test function names.
- Do not remove apparently duplicated tests during the split.
- Compare `cargo test --lib -- --list` before and after.
- Do not move these into repository-level integration tests.

## 2. Recording Editor Model (Complete)

Current file: `src/recording/editor/model_parts/types.rs` (about 1,866 lines).

The safest split preserves the existing textual `include!` architecture and keeps everything inside the same logical Rust module. This avoids changing public paths or widening private methods.

```text
src/recording/editor/model_parts/editor_types.rs
src/recording/editor/model_parts/cursor_types.rs
src/recording/editor/model_parts/zoom_types.rs
src/recording/editor/model_parts/motion_transform.rs
src/recording/editor/model_parts/motion_text.rs
src/recording/editor/model_parts/motion_blur.rs
src/recording/editor/model_parts/motion_scene.rs
src/recording/editor/model_parts/motion_state_impl.rs
src/recording/editor/model_parts/media_types.rs
```

Suggested ownership:

- `editor_types.rs`: metadata, dimensions, audio, backgrounds, and tools
- `cursor_types.rs`: cursor themes, effects, motion presets, and settings
- `zoom_types.rs`: `ZoomClip` and zoom formatting helpers
- `motion_transform.rs`: transforms, easing, segments, and interpolation
- `motion_text.rs`: text styles, animations, scope, and sampling
- `motion_blur.rs`: blur settings, samples, and budgets
- `motion_scene.rs`: appearance, watermark, and the `MotionState` declaration
- `motion_state_impl.rs`: timeline editing, snapping, reconciliation, and sampling
- `media_types.rs`: crop, project media, and `VideoEditState`

Critical invariants:

- Preserve `recording::editor::model::*` paths.
- Preserve inclusive selection boundaries and exclusive overlap boundaries.
- Keep every transform mutation followed by segment reconciliation.
- Keep selection indexes valid after sorting, insertion, truncation, and deletion.
- Do not convert these into independent Rust modules in the initial extraction.

## 3. Editor CSS (Complete)

Current file: `src/capture/editor/editor.css` (about 3,185 lines).

Split into ordered fragments:

```text
src/capture/editor/css/01-shell-toolbar.css
src/capture/editor/css/02-crop-controls.css
src/capture/editor/css/03-tools.css
src/capture/editor/css/04-color-palette.css
src/capture/editor/css/05-color-picker.css
src/capture/editor/css/06-text-actions.css
src/capture/editor/css/07-footer-floating.css
src/capture/editor/css/08-zoom-popup.css
src/capture/editor/css/09-theme-overrides.css
src/capture/editor/css/10-canvas-inspector.css
src/capture/editor/css/11-background-sidebar.css
src/capture/editor/css/12-background-choices.css
src/capture/editor/css/13-text-modal.css
```

Load the fragments through one aggregate constant:

```rust
const EDITOR_CSS: &str = concat!(
    include_str!("css/01-shell-toolbar.css"),
    include_str!("css/02-crop-controls.css"),
    // Remaining files in exact original order.
);
```

Critical invariants:

- Keep one `CssProvider`.
- Keep `STYLE_PROVIDER_PRIORITY_USER`.
- Preserve exact stylesheet order.
- Do not deduplicate repeated light-theme or zoom rules yet.
- Do not rename shared `.editor-*` classes during extraction.

## 4. Settings CSS (Complete)

Current file: `src/settings/settings.css` (about 2,233 lines).

```text
src/settings/css/01-shell-controls.css
src/settings/css/02-sidebar-actions.css
src/settings/css/03-shared-content.css
src/settings/css/04-native-widgets.css
src/settings/css/05-tabs-modes-shortcuts.css
src/settings/css/06-shortcut-dialog.css
src/settings/css/07-pages-about-onboarding.css
src/settings/css/08-recent-captures.css
src/settings/css/09-noir-gallery.css
src/settings/css/10-history-shell.css
```

Critical invariants:

- Keep one provider at `STYLE_PROVIDER_PRIORITY_APPLICATION`.
- Keep history rules after recent-capture rules.
- Preserve the broad `.editor-root button` rules before component overrides.
- Do not consolidate duplicated traffic-control rules during this phase.
- Test opening Settings, History, the image editor, and the recording editor in different orders because providers are display-global.

## 5. Motion Renderer (Complete)

Current file: `src/capture/editor/window/motion_render.rs` (about 1,918 lines).

Retain `motion_render.rs` as a facade so existing imports continue working.

```text
src/capture/editor/window/motion_render.rs
src/capture/editor/window/motion_render/geometry.rs
src/capture/editor/window/motion_render/background.rs
src/capture/editor/window/motion_render/card.rs
src/capture/editor/window/motion_render/overlays.rs
src/capture/editor/window/motion_render/compositor.rs
src/capture/editor/window/motion_render/export.rs
```

Symbol ownership:

- `geometry.rs`: `MotionStage`, `CardLayout`, projection, and coordinate conversion
- `background.rs`: backdrop, noise, cover-fit, blur, and image loading
- `card.rs`: rounded card, shadow, perspective mesh, and triangle painting
- `overlays.rs`: text, watermark, reveal, and hit testing
- `compositor.rs`: `draw_motion_frame` and layer ordering
- `export.rs`: MP4 rendering and unique output paths

Critical invariants:

- Preview and export must continue sharing `draw_motion_frame`.
- Preserve layer order: backdrop, historical samples, sharp card, text, watermark.
- Keep one geometry implementation for both painting and hit testing.
- Preserve preview-versus-export quality and checkerboard behavior.
- Move model-behavior tests to model ownership only after the rendering split is stable.

## 6. Capture Overlay Rust API (Complete)

Current file: `src/capture_overlay.rs` (about 2,505 lines).

Keep `capture_overlay.rs` as a compatibility facade with re-exports.

```text
src/capture_overlay/types.rs
src/capture_overlay/environment.rs
src/capture_overlay/session.rs
src/capture_overlay/binary_locator.rs
src/capture_overlay/process.rs
src/capture_overlay/worker.rs
src/capture_overlay/protocol.rs
src/capture_overlay/errors.rs
src/capture_overlay/image_io.rs
src/capture_overlay/arguments.rs
src/capture_overlay/wlroots.rs
src/capture_overlay/portal.rs
src/capture_overlay/recording_controls.rs
src/capture_overlay/api.rs
```

Extraction sequence:

1. Public protocol types
2. Argument builders
3. Protocol parsing
4. Image conversion
5. Binary discovery
6. Error formatting
7. Session coordination
8. Warm worker
9. Process execution
10. Platform adapters
11. High-level API

Critical invariants:

- Re-export all existing public types and functions.
- Preserve native-helper exit-code semantics.
- Preserve warm-worker failure fallback to cold execution.
- Keep process and session guards alive until jobs complete.
- Preserve binary search precedence.
- Preserve ownership of temporary files.
- Do not replace manual parsing with serde during the structural split.
- Do not change legacy sentinel-coordinate behavior yet.

## 7. Motion Mode UI

Current file: `src/capture/editor/window/motion_mode.rs` (about 3,612 lines).

Retain `motion_mode.rs` as a facade containing page constants and re-exports.

```text
src/capture/editor/window/motion_mode/session.rs
src/capture/editor/window/motion_mode/parts.rs
src/capture/editor/window/motion_mode/build.rs
src/capture/editor/window/motion_mode/appearance.rs
src/capture/editor/window/motion_mode/watermark.rs
src/capture/editor/window/motion_mode/widgets.rs
src/capture/editor/window/motion_mode/preview.rs
src/capture/editor/window/motion_mode/transition.rs
src/capture/editor/window/motion_mode/controls/mod.rs
src/capture/editor/window/motion_mode/controls/sync.rs
src/capture/editor/window/motion_mode/controls/playback.rs
src/capture/editor/window/motion_mode/controls/timeline.rs
src/capture/editor/window/motion_mode/controls/text.rs
src/capture/editor/window/motion_mode/controls/transform.rs
```

Important first steps:

1. [x] Move `MotionRuntime` and `MotionSession` into `session.rs`.
2. [x] Divide `MotionModeParts` into grouped parts structures.
3. [x] Extract panel and widget builders.
   - [x] Extract reusable widget builders into `widgets.rs`.
   - [x] Extract the appearance inspector and background picker into `appearance.rs`.
   - [x] Extract the watermark panel builder into `watermark.rs`.
4. [x] Extract mode transition and confirmation handling.
5. [x] Extract Motion page construction into `build.rs`.
6. [x] Extract preview rendering setup into `preview.rs`.
7. [x] Split the roughly 1,400-line `wire_motion_controls` function into focused control installers.

Critical invariants:

- Continue using `Rc<RefCell<_>>`; do not introduce threading primitives.
- Preserve `inspector_syncing` feedback-loop prevention.
- Drop runtime borrows before updating widgets or triggering callbacks.
- Keep background and watermark model paths synchronized with cached Cairo surfaces.
- Preserve callback installation order and ensure callbacks are installed once.
- Preserve playback timing, end-of-playback behavior, and the 33 ms timer.
- Preserve snapshot capture and destructive cleanup order when changing modes.

## 8. Editor Window Root

Current file: `src/capture/editor/window/mod.rs` (about 3,063 lines).

Do this after `motion_mode` has a smaller interface.

Add:

```text
src/capture/editor/window/motion_host.rs
```

`motion_host.rs` should own:

- Motion page creation
- Session and runtime setup
- Inspector and toolbar tab wiring
- Enter and leave callbacks
- Motion control installation
- Export callback construction

Keep in `mod.rs`:

- General editor window construction
- Static editor setup
- Composition of static and Motion surfaces
- Passing `in_motion` and the export callback to output handling

Do not make general output code depend directly on Motion runtime types.

## 9. C++ Drawing

Current file: `capture-overlay/src/CaptureOverlay_Drawing.cpp` (about 2,110 lines).

```text
capture-overlay/src/CaptureOverlay_DrawingPrimitives_p.h
capture-overlay/src/CaptureOverlay_DrawingPrimitives.cpp
capture-overlay/src/CaptureOverlay_Layout.cpp
capture-overlay/src/CaptureOverlay_Paint.cpp
capture-overlay/src/CaptureOverlay_RecordingDrawing.cpp
capture-overlay/src/CaptureOverlay_RecordingSettingsDrawing.cpp
capture-overlay/src/CaptureOverlay_ToolbarDrawing.cpp
capture-overlay/src/CaptureOverlay_Audio.cpp
capture-overlay/src/CaptureOverlay_ScrollPopupDrawing.cpp
```

Safe sequence:

1. [x] Audio process and volume popup
2. [x] Scroll popup
3. [x] Generic dropdown
4. [x] Recording settings
5. [x] Recording panel
6. [x] Capture toolbar
7. [x] Drawing primitives
8. [x] Shared layout definitions
9. [x] `paintEvent`

Critical invariants:

- Keep functions as `CaptureOverlay` member definitions initially.
- Preserve `m_settingsClickableRects` numeric index ordering.
- Preserve `m_recTileRects` append order.
- Clear and repopulate popup and menu rectangles on the same paint paths.
- Keep external linkage for `computeToolbarLayout`.
- Keep exactly one definition of toolbar arrays.
- Add every new `.cpp` to `capture-overlay/CMakeLists.txt`.
- Add matching `cargo:rerun-if-changed` entries to `build.rs`.
- Do not remove apparently unused helpers in the extraction commit.

## 10. Recording Backend

Current file: `src/recording/backend.rs` (about 2,616 lines).

This should be last because shutdown, drop order, FFmpeg arguments, and descriptor ownership are behaviorally significant.

```text
src/recording/backend/session.rs
src/recording/backend/source.rs
src/recording/backend/crop.rs
src/recording/backend/profile.rs
src/recording/backend/ffmpeg_process.rs
src/recording/backend/wayland.rs
src/recording/backend/x11.rs
src/recording/backend/gif.rs
```

Keep `backend.rs` as a facade containing or re-exporting:

- `BuiltPipeline`
- `PreparedGifWaylandRecording`
- `prepare_recording_backend`
- `start_recording_with_prepared_backend`
- Paths consumed by `controls.rs`, `wf_recorder.rs`, and audio tests

Extraction sequence:

1. Crop and sizing helpers
2. Encoder profiles
3. FFmpeg process helpers
4. Session ownership
5. Wayland source acquisition
6. X11 recording
7. GIF recording
8. Wayland recording loop intact
9. Optional later decomposition of the Wayland loop

Critical invariants:

- Preserve portal, PipeWire, and audio resource drop order.
- Preserve FFmpeg argument ordering.
- Keep control handling active during blocked frame writes.
- Preserve descriptor-3 ownership and `CLOEXEC` handling.
- Preserve save, discard, and restart busy-state semantics.
- Preserve audio and video start and shutdown alignment.
- Keep `wayland_video_filter` available to `wf_recorder`.
- Do not redesign the roughly 586-line Wayland loop while first moving it.

## Verification Gates

After every extraction batch:

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo check --all-targets --no-default-features --features flatpak
cargo clippy --workspace --all-targets
```

Focused tests:

```bash
cargo test --lib -- capture::editor --test-threads=1
cargo test --lib -- capture_overlay --test-threads=1
cargo test --lib -- recording::editor::model --test-threads=1
cargo test --lib -- recording::backend --test-threads=1
cargo test --test package_metadata
```

Final repository gate:

```bash
cargo test --jobs 2 -- --test-threads=1
cargo build --release --verbose
```

For the C++ helper, also perform a clean CMake build:

```bash
cmake -S capture-overlay -B /tmp/apexshot-capture-build
cmake --build /tmp/apexshot-capture-build
```

## Manual Regression Checks

Automated tests do not fully cover GTK, compositor, capture, and rendering behavior. After the related phases, verify:

- Editor and Settings in dark and light themes
- Reduced-transparency mode
- Editor fullscreen and normal window modes
- Crop, color, text, background, zoom, Motion, and inspector controls
- Settings, History, onboarding, and shortcut dialogs
- Motion playback, timeline editing, text dragging, preview, and MP4 output
- Preview and export agreement for transforms, text, watermark, blur, shadow, and backgrounds
- Warm and cold native capture helper paths
- Area, crosshair, fullscreen, scroll, OCR, and recording modes
- Overlay controls and hit targets at multiple display scales
- GNOME, KDE, wlroots, X11, and portal paths where available
- Recording pause, resume, restart, save, discard, audio, GIF, and area cropping

## Change Policy

Each refactoring batch should follow these rules:

1. Move code without modifying behavior.
2. Keep old module paths through facades and re-exports.
3. Move associated tests with the code when practical.
4. Run focused tests immediately after each extraction.
5. Keep source order where order has meaning.
6. Do not mix deletion, deduplication, or redesign into extraction changes.
7. Perform manual visual and capture checks after CSS, rendering, and C++ drawing splits.
