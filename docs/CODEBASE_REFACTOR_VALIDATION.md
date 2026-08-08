# Codebase refactor implementation validation

Date: 2026-08-04  
Corrections applied: 2026-08-05  
PR 10+ plan added: 2026-08-06
Validated documents: [`CODEBASE_REFACTOR_AUDIT.md`](./CODEBASE_REFACTOR_AUDIT.md) and [`CODEBASE_REFACTOR_STATUS.md`](./CODEBASE_REFACTOR_STATUS.md)  
Scope: Current working-tree implementation of the `src/` refactor described as PRs 0-9.

---

## Executive verdict

The mechanical cleanup and structural splitting through PR 9 now match the audit, preserve their intended facades, and pass the core Rust test suites. The large GTK coordinators were correctly left in place rather than moved wholesale.

**Post-correction (2026-08-05):** the blocking validation findings are fixed:

- [x] PR 9 daemon IPC string regressions (`open_file`, preview D-Bus member, hotkey name match) restored with protocol unit tests.
- [x] `find_physical_input_device` narrowed back to `pub(crate)`.
- [x] Desktop-identity and package-metadata contract tests updated for `src/cli/install.rs` and current packaging reality.
- [x] PR 0 Clippy `needless_return` and Flatpak Tesseract dead-code warnings cleared.
- [x] PR 6 implementation tests moved to `backend.rs` and `controls.rs`; test-driven sibling visibility removed.
- [x] Status-document ownership/API descriptions and `EDITOR_CSS` visibility corrected.
- [x] PR 10 began after this validation snapshot with initial state, editor-event, editor-inspector, overlay-window, and daemon-dispatch slices.
- [ ] Manual GTK/D-Bus/portal/capture smoke matrix still unverified.
- [ ] PR guardrails still cannot be proven from commit history (single dirty working tree).

No source implementation was changed during the original validation pass. Corrections above were applied afterward.

---

## Status by planned PR

| Work item | Validation result | Assessment | Correction |
|-----------|-------------------|------------|------------|
| PR 0: clean gates | **Done** | Format, check, Clippy `-D warnings`, Flatpak check, and contract tests pass. | [x] |
| PR 1: dead code | Confirmed | The orphan and listed internal dead code were removed without deleting the live editor text paths. | n/a |
| PR 2: settings CSS | Confirmed | CSS was moved byte-for-byte and loaded through `include_str!`. | n/a |
| PR 3: editor CSS/tests | Confirmed | CSS and tests were moved correctly; targeted editor tests pass. | n/a |
| PR 4: render split | Confirmed | Module ownership, function bodies, and internal facade are preserved. | n/a |
| PR 5: overlay drawing split | Confirmed | Split is sound; status ownership wording corrected. | [x] docs |
| PR 6: recording split | **Confirmed** | Runtime and test ownership splits are complete; internal test helpers use minimum visibility. | [x] |
| PR 7: CLI split | **Confirmed** | Binary-only module split is correct. Contract tests now inspect `src/cli/install.rs`. | [x] |
| PR 8: hotkeys split | Confirmed | Platform/config ownership and facade are preserved. | n/a |
| PR 9: daemon split | **Confirmed after fix** | Physical split is coherent; IPC strings and crate-private API restored. | [x] |
| PR 10+ | **In progress** | Track A, events Track B, and setup Track C (10.11–10.14) done; interaction slices (Track D) and overlay remain. | in progress |
| `capture_overlay.rs` | Deferred | It remains a single 2,431-line file with no child module tree. | deferred |

---

## PR 10+ implementation plan

### Planning baseline (2026-08-06)

This plan supersedes the earlier statements in this validation snapshot that PR 10 had not started. The live tree now matches the newer status document:

- `src/capture/editor/state.rs` is now `state/`, with `text_input.rs`, `history.rs`, `drag_draw.rs`, and `export.rs` extracted.
- `src/capture/editor/window/events.rs` is now `events/`, with zoom and history-button wiring extracted.
- `window/inspectors/` owns `InspectorParts` plus family panel builders (`select`, `crop`, `stroke`, `text`, `number`, `obfuscate`).
- `src/overlay/window.rs` is now `window/`, with audio primitives and platform policy extracted.
- `run_daemon_inner` is already a short coordinator after action dispatch moved to `daemon/dispatch.rs`; no further daemon split is required for PR 10.

The remaining logic-bearing coordinators are:

| Coordinator | Current location | Approximate size | Target |
|-------------|------------------|-----------------:|--------|
| `wire_editor_events` | `capture/editor/window/events/mod.rs` | ~2,150 lines | Ordered dispatcher over behavior-owned event modules |
| `setup_editor_window_full` | `capture/editor/window/mod.rs` | 2,990 lines | Bootstrap coordinator over concrete builders and services |
| `overlay::window::setup_window` | `overlay/window/mod.rs` | 1,850 lines | Lifecycle coordinator over shell, result, audio, and input owners |

Line count is a secondary signal. Completion requires real ownership, one-way dependencies, stable facades, and no replacement god-context structs.

### Scope and non-goals

In scope:

- Finish splitting `EditorState` impls by behavior while keeping `EditorState` and its fields in `state/mod.rs`.
- Extract editor event families only after their state and widget dependencies have stable owners.
- Extract setup sections only when they return concrete widgets/results or own a complete asynchronous service.
- Apply the same behavior-slice rule to the overlay window coordinator.
- Move source-inspection tests with the implementation they assert.

Not in scope for the movement PRs:

- Changing editor, overlay, capture, save, or recording behavior.
- Moving `EditorState` fields into nested state objects or privatizing fields used directly by GTK code.
- Introducing a new all-purpose `EditorUiContext`, `OverlayEventContext`, or callback bag.
- Changing public editor/overlay entry points, crate re-exports, IPC names, D-Bus members, or result variants.
- Adding dependencies, redesigning asynchronous effects, or combining packaging/recording work with GTK movement.
- Splitting `capture_overlay.rs` before the GTK coordinator sequence is stable.

### Required PR shape

Every numbered slice below is independently reviewable. If several slices share a branch, retain the same commit discipline:

1. Add characterization tests or fix a known correctness precondition in a focused commit.
2. Move production code without changing bodies, signatures, callback order, or visibility.
3. Move/update tests so they inspect the new owner rather than concatenating unrelated source files.
4. Narrow visibility or simplify dependencies only in a follow-up commit.
5. Run the affected focused tests and the common PR gate before starting the next slice.

### Track A: finish `EditorState` ownership

Keep `EditorState`, `TextInputState`, `EditorState::new`, `set_tool`, and `set_tool_without_rebuild` in `state/mod.rs`. Tool transitions intentionally coordinate crop, selection, text, arrow, and drag cleanup.

#### PR 10.1: arrow editing state — **Done (2026-08-06)**

Created `state/arrow.rs` and moved the existing-arrow behavior:

- Arrow style getters/setters and selected-arrow style mutation.
- Arrow reversal.
- Control-handle hit testing and movement.
- Arrow control finalization and interaction cleanup.

Kept new-arrow construction in `drag_draw.rs`. Moved the two arrow unit tests into `state/arrow.rs`. Method signatures unchanged.

Exit gate: `cargo test --lib -- capture::editor` (211 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.2: crop state — **Done (2026-08-06)**

Created `state/crop.rs` and moved:

- Crop aspect-ratio and resize geometry helpers.
- Crop selection initialization, drag, resize, reset, and apply behavior.
- Crop fill/image helpers and `draft_crop_rect` from `drag_draw.rs`.
- Crop-specific test `reset_crop_interaction_clears_crop_selection_and_drag_handles`.

`crop_image` is `pub` inside the private `crop` module; `export.rs` imports `super::crop::crop_image`. Freeform/fixed-ratio clamping and post-apply reset unchanged.

Exit gate: `cargo test --lib -- capture::editor` (211 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.3: text-state consolidation — **Done (2026-08-06)**

Moved the remaining text-specific methods from `state/mod.rs` into `state/text_input.rs`:

- Text size and font-family selection/mutation.
- Selected-text data lookup.
- Active text commit and selected-text editing startup.
- Test-only text update and production `cancel_text_edit`.

Kept `TextInputState` in `state/mod.rs`. Preserved `active_text_edit`, `active_text_entry`, and Escape cancellation path.

Exit gate: `cargo test --lib -- capture::editor` (211 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.4: selection state — **Done (2026-08-07)**

Created `state/selection.rs` and moved:

- Selected-action lookup and removability.
- Selection hit testing, drag, resize, completion, and deletion.
- Selected-action color/stroke mutation.
- `clamp_action_to_image` and the related tests.

Kept `capture/editor/selection.rs` as the geometry/hit-test primitive owner; state mutation may depend on those primitives, never the reverse.

Exit gate: topmost selection, zoom-scaled hit padding, all resize handles, movement, deletion, number reuse, arrow hit testing, and effect-dirty behavior pass under `cargo test --lib -- capture::editor`.

#### PR 10.5: tool-style state — **Done (2026-08-07)**

Created `state/tool_style.rs` and moved:

- Active color and stroke size.
- Obfuscation method/amount and focus intensity.
- Active size-control mode/value and selected-action style mutation not already owned by text, crop, or arrow modules.

Did not nest style fields into a new struct. Numbering remains in `state/mod.rs`. Pen/highlighter weight setters currently live on `export.rs` (co-located with render helpers); no nested style object was introduced.

Exit gate: clamping, selected-object updates, focus intensity, blur/pixelate/blackout, and pen/highlighter tests pass under `cargo test --lib -- capture::editor`.

#### PR 10.6: optional effect/export helper cleanup — **Done (2026-08-07)**

Moved effect rebuild/application helpers to `state/effects.rs` and kept export-only crop/shadow/corner helpers in `export.rs`. Preserved the internal `state` facade used by editor window code (`apply_effect_actions`, `render_shadow_layer`).

### Track B: extract low-risk editor event families

Keep `events::wire_editor_events` as the only setup-facing facade and preserve GTK controller installation order. Remove unused `EventContext` fields before adding any new context data; do not replace it with another larger bag.

#### PR 10.7: output lifecycle — **Done (2026-08-07)**

Created `events/output.rs` for copy, upload, Done/save, and traffic-close behavior. Preserved:

- Save-before-upload ordering and double-click suppression.
- Window hide before idle save and re-show on failure.
- Nonfatal annotation-save failure.
- Clipboard, preview-daemon fallback, close, and application-quit ordering.
- Session guards on stale window callbacks.

Moved the upload source-contract test to this owner.

#### PR 10.8: crop action buttons — **Done (2026-08-08)**

Created `events/crop.rs` with `wire_crop_action_buttons` for crop apply/reset buttons and their dimension/layout refresh. Bodies moved unchanged:

- Apply calls `apply_crop_selection`, refreshes canvas content size on success, clears apply-button enabled selection state, and updates crop size fields.
- Reset calls `reset_crop_interaction` and performs the same button/field/redraw refresh.

Canvas crop dragging remains in the interaction controller. Structure test `crop_action_buttons_refresh_layout_and_fields` lives on this owner.

Exit gate: `cargo test --lib -- capture::editor` (215 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.9: tool mode switches — **Done (2026-08-08)**

Created `events/tools.rs` with `wire_tool_mode_switches` + `ToolModeButtons` for Select, Crop, Background, Pen, Arrow, Line, Box, Circle, Text, Number, Obfuscate, Focus, and Highlighter activation. Distinct toggle policies retained:

- Crop toggles off to Arrow and calls `ensure_crop_selection_initialized`.
- Background / Number / Highlighter toggle-off policies.
- Obfuscate clears `select_effect_rebuild_pending` and may rebuild when effect actions exist.
- Pen updates the pen cursor.

Identical non-toggling tools (Arrow/Line/Box/Circle/Text/Focus) share a private `wire_standard_tool` helper with the same callback body. Structure test `tool_mode_switches_preserve_special_toggle_policies` lives on this owner. `production_events_source` in `events/mod.rs` now also includes `output.rs` and `tools.rs`.

Exit gate: `cargo test --lib -- capture::editor` (214 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.10: tool options — **Done (2026-08-08)**

Created `events/options.rs` with `wire_tool_options` + `ToolOptionsParts` for:

- Pen/highlighter weight lists (including freehand mode + pen cursor update)
- Obfuscate method list (icon update, effect rebuild, size-control sync)
- Arrow style, stroke size, and arrow thickness lists
- Inverse-direction toggle
- Number style, start +/- controls, and number size
- Color palette (crop background / plain background / tool color)
- Size slider (`set_active_size_without_rebuild` + effect rebuild)

Moved `sync_arrow_option_selection` and `sync_number_option_selection` with the handlers. Left text size/font list *sync* in `mod.rs` for canvas click re-edit. Structure test `tool_options_cover_weight_style_numbering_palette_and_size` lives on this owner.

Exit gate: `cargo test --lib -- capture::editor` (216 passed); fmt/check/clippy/flatpak gates green.

### Track C: give editor setup sections concrete owners

Use the existing `ToolbarParts`, `CanvasParts`, `FooterParts`, and `InspectorParts` pattern. A Parts type contains concrete widgets/results only, not state, arbitrary callbacks, or unrelated services.

#### PR 10.11: inspector children — **Done (2026-08-08)**

Converted `window/inspectors.rs` → `window/inspectors/` with family-owned panel builders:

- `shell.rs` — shared `build_tool_inspector` / `append_inspector_section`
- `select.rs` — selection inspector
- `crop.rs` — crop dimensions/aspect/actions
- `stroke.rs` — pen, arrow, line, highlighter thickness/style panels
- `text.rs` — size/font
- `number.rs` — style/start/size
- `obfuscate.rs` — method
- `mod.rs` — shell facade (`build_tool_inspectors` → `InspectorParts`), stack/tabs/sidebar actions

Setup still calls the same `inspectors::build_tool_inspectors(InspectorContentInputs { ... })` entry. Child builders take narrower domain input structs internally. Source-inspection helper concatenates the inspector child tree.

Exit gate: `cargo test --lib -- capture::editor` (217 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.12: asynchronous effects service — **Done (2026-08-08)**

Created `window/effects.rs` with `install_async_effects_pipeline(state, drawing_area) -> Rc<dyn Fn()>` owning:

- Request/result `mpsc` channels
- 16ms UI-thread result poll with stale-revision rejection
- Dirty-flag coalescing when a rebuild is already pending
- Single worker thread that drains to the latest request and runs `apply_effect_actions`
- 500ms watchdog recovering stuck `select_effect_rebuild_pending` after 2s
- The `rebuild_effects_async` callback returned to setup/events

Revision fields, timer intervals, and lock lifetimes preserved. GTK widgets stay on the main thread. Setup now installs the service in one call.

Exit gate: `cargo test --lib -- capture::editor` (218 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.13: background assets and canvas layout — **Done (2026-08-08)**

Created two separate owners (not combined):

- `window/background_assets.rs` — `install_background_asset_loading` → `BackgroundAssetCaches` (gradient slots, wallpaper cache, loader sender). Preloads system wallpaper + gradient assets on a worker; 100ms UI poll inserts surfaces and redraws.
- `window/canvas_layout.rs` — `install_canvas_layout` → `update_canvas_content_size`. Owns fit/scale math, background virtual size via `BackgroundComposition`, crop overflow, zoom label updates, and the scroller tick with capped-overflow signature to suppress relayout churn.

Setup installs each service independently. Draw path still consumes the asset cache handles.

Exit gate: `cargo test --lib -- capture::editor` (220 passed); fmt/check/clippy/flatpak gates green.

#### PR 10.14: empty state and chrome — **Done (2026-08-08)**

Created:

- `window/chrome.rs` — `install_window_chrome`: top drag strip hosting toolbar, floating zoom/history overlays, window drag + edge resize installation.
- `window/empty_state.rs` — `install_empty_drop_zone` plus supported-file checks, open dialog, async same-window load, drop target, and loading banner.

Session reuse / stale controller removal remains in setup. `setup_editor_window_full` stays the reload target for empty→loaded transitions.

Exit gate: `cargo test --lib -- capture::editor` (222 passed); fmt/check/clippy/flatpak gates green.

### Track D: split high-coupling editor interactions

Start this track only after Tracks A-C provide stable state APIs, widget parts, and service callbacks.

#### PR 10.15: interaction state

Create small cohesive owners such as `SpacePanState` and an eyedropper bundle. Capture and reuse the callback returned by `wire_zoom_controls`; preserve the existing `Ctrl+2` behavior during structural movement.

#### PR 10.16: canvas drag

Create `events/drag.rs` and move the complete begin/update/end gesture family together. Preserve controller attachment order, redraw throttling, crop refresh, effect rebuild flags, arrow controls, selection behavior, and Space-pan interaction.

#### PR 10.17: canvas click

Create `events/click.rs` for click press/release, canvas Escape, text create/re-edit/caret placement, eyedropper completion, number placement, and text-handle release. Do not split these further until shared hit testing has a clear owner.

#### PR 10.18: motion and keyboard

Use separate movement commits/modules:

- `events/motion.rs` owns cursor/loupe/hover and active text-handle resize behavior.
- `events/keyboard.rs` owns Space capture, text input, undo/redo, zoom, tool shortcuts, deletion, and Escape priority.

Preserve controller propagation phases, text-widget exceptions, Enter-as-newline behavior, and existing shortcut mappings.

#### PR 10.19: canvas renderer and bootstrap last

Extract the `set_draw_func` body only after render inputs have a concrete owner. Introduce rendering-specific cache/snapshot types, release the `EditorState` lock before Cairo drawing, and preserve all cache signatures, transforms, crop dimming, effect skipping, editing overlays, and handles.

Session/bootstrap extraction is last. Preserve the lifetime of `ACTIVE_EDITOR_SESSION`, window reuse, preference persistence, always-on-top behavior, and public `open_image_editor*` / `setup_editor_window` entry points.

### Track E: overlay window coordinator

Overlay work is independent from editor state and should use separate PRs/branches. Preserve all public `overlay` and crate-root exports, `SelectionResult` variants, coordinate-space distinctions, layer-shell policy, and the crate-internal drawing/layout facade used by `recording/stop_overlay.rs`.

#### PR 10.20: correctness preconditions and shared geometry

Before moving callbacks:

- Add a regression test preventing toolbar hit testing from returning an index outside `TOOLBAR_ICONS`; fix the current seven-cell/six-icon mismatch in a focused correctness commit.
- Characterize/fix the timer badge index separately.
- Move aspect-ratio behavior to overlay geometry and add freeform, fixed-ratio, center, edge-clamp, and minimum-size tests.
- Centralize popup/deck/settings rectangles so drawing and hit testing consume the same layouts.

Do not silently remove dormant window-picker state; the public API and `window_picker_ui_contract` still constrain it.

#### PR 10.21: overlay result and shell

Create:

- `overlay/window/result.rs` for recording-request construction and selection result delivery.
- `overlay/window/shell.rs` for monitor resolution, window/layer-shell setup, drawing area, and screen geometry, returning concrete `OverlayWindowParts`.

Keep realization, focus, and presentation in `window/mod.rs`. Preserve screenshot/background coordinate mapping, recording screen coordinates, OCR conversion, invalid-selection behavior, and Escape returning `Area(None)`.

#### PR 10.22: overlay audio lifecycle

Move meter worker/timer orchestration into the existing `window/audio.rs` without behavior changes. In a separate follow-up, add explicit cancellation so closing an overlay cannot leave a polling thread or GLib source retaining state.

#### PR 10.23: overlay keyboard and countdown

Create `window/input/keyboard.rs` and `window/countdown.rs`. Preserve Escape, Enter variants, Space confirmation, arrow-key nudge, propagation results, timer cancellation, redraw scheduling, and final result delivery.

#### PR 10.24: overlay drag

Create `window/input/drag.rs` after result/aspect helpers are stable. Move the complete drag gesture and preserve offset semantics, fixed-aspect resize behavior, popup/tool-surface suppression, and lock release before result delivery or window closure.

#### PR 10.25: overlay motion

Create `window/input/motion.rs` after shared popup geometry exists. Move motion and leave/reset behavior together, preserving hover priority, cursor selection, crosshair updates, and popup/slider ownership.

#### PR 10.26: overlay click dispatch last

Create `window/input/click/` with menu and toolbar owners. First move the primary/secondary gesture bodies unchanged; then split pure state transitions from GTK, URL, channel, and close side effects. Preserve capture-phase controller ordering and never hold `SelectorState` across those side effects.

At completion, `overlay::window::setup_window` should only build the shell, wire drawing/audio/input, install the X11 realization hook, focus, and present.

### Dependency and visibility rules

Required dependency direction:

```text
editor/window/mod
  -> concrete builders and services
  -> events facade
  -> event-family modules
  -> EditorState behavior methods
  -> editor geometry/render/color primitives

overlay/window/mod
  -> shell/result/audio/countdown/input
  -> overlay state/layout/hit-testing/drawing primitives
```

Rules:

- State primitives must not depend on `window` or `events`.
- Builders must not call event modules; event modules consume their concrete Parts.
- Event siblings communicate through parent-owned small types, not sibling cycles.
- New modules are private by default; use `pub(super)` only for the immediate facade.
- Preserve existing facade paths where sibling or crate-internal callers require them.
- Do not add crate-root/capture-level re-exports or broaden downstream API.
- Keep GTK widgets on the main context and out of `Send` worker payloads.
- Release state mutexes before GTK calls, Cairo rendering, D-Bus/process/URL work, channel sends, or window closure.
- Preserve GTK controller attachment order and `PropagationPhase::Capture` where currently used.

### Test ownership

Source-inspection tests must follow semantic ownership:

- State behavior tests move into the owning state child when that permits helper visibility to remain private.
- Inspector structure tests move under `window/inspectors/`.
- Layout/render assertions move to `canvas_layout.rs`, `canvas.rs`, or `canvas_render.rs`.
- Zoom, output, Space-pan, text-key, and upload-order tests move to their event owners.
- Overlay click contracts inspect the actual click owner after movement; `window_picker_ui_contract` must not keep reading only a facade that no longer contains the behavior.

Only coordinator ordering tests should continue to inspect `mod.rs`. Do not solve path-sensitive tests by concatenating every child source into one large string.

### Automated gates

Run after every slice:

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo check --all-targets --no-default-features --features flatpak
cargo clippy --all-targets -- -D warnings
```

Run focused suites for the affected track:

```bash
cargo test --lib -- capture::editor
cargo test --lib -- overlay::
cargo test --lib -- recording::stop_overlay
cargo test --test window_picker_ui_contract
```

Run before each merge-ready milestone:

```bash
cargo test --all-targets
git diff --check
```

For pure movement commits, compare moved function bodies after normalizing indentation and verify that public/crate-internal facade paths have not changed.

### Manual exit matrix

Editor milestone:

- Open an existing image from CLI, preview overlay, and daemon paths; also exercise the empty editor and drag/drop.
- Exercise every tool, inspector, palette, custom color, eyedropper, zoom/pan path, and keyboard shortcut.
- Draw/select/move/resize/delete/undo/redo, including curved/double arrows and multiline text re-edit/cancel.
- Apply/reset freeform and fixed-aspect crops, including out-of-bounds movement.
- Rapidly change obfuscation/focus and undo/redo while effects rebuilds are in flight.
- Verify backgrounds, shadows, copy, upload, save failure recovery, preview fallback, close, and preference persistence.

Overlay milestone:

- Test X11, Wayland with layer-shell, and Wayland fallback where available, including multiple monitors.
- Test new/move/resize/fixed-aspect/fullscreen selection, nudge, crosshair, confirmation keys, Escape, and countdown.
- Exercise every capture toolbar popup and recording panel/settings/dropdown/volume path.
- Verify screenshot versus recording coordinate mapping and daemon-present/absent audio meters.
- Confirm close leaves no PipeWire streams, polling threads, or repeating GLib sources.

Final PR 10+ exit gate:

- Full capture -> editor -> annotate -> undo/redo -> crop -> save.
- Settings open/save/close.
- Daemon hotkey and record start/pause/resume/stop.
- No logic-bearing coordinator remains over 1,000 lines solely because behavior still lacks an owner.
- No new dependency, downstream public API, IPC string, D-Bus member, or result-contract change.

### Deferred follow-up

After the editor and overlay coordinators settle, plan `capture_overlay.rs` as a separate process-boundary series: worker lifecycle, output parsing, wlroots routing, and recording-request spawn. Do not include that split in the same PR as GTK coordinator movement.

---

## Findings

### High: `open-file` daemon dispatch is broken — **FIXED**

The CLI sends the stable action name `"open_file"`:

- `src/main.rs:204-208`

**Was broken:** the daemon matched only `"capture_handlers::open_file"`.

**Fix:** restored stable protocol parsing via `parse_trigger_action` in `src/daemon/mod.rs`, matching `"open_file"`. Added `parse_trigger_action_accepts_stable_open_file_protocol_name` so Rust module paths cannot become IPC names.

### High: editor preview D-Bus calls use an invalid moved-function path — **FIXED**

**Was broken:** `show_preview_via_daemon` called `"capture_handlers::show_preview_for_path"`.

**Fix:** client now calls the stable member through `SHOW_PREVIEW_FOR_PATH_MEMBER` (`"show_preview_for_path"`), matching the zbus server method. Covered by `preview_dbus_member_is_stable_method_name`.

### Medium: hotkey name matching was corrupted during extraction — **FIXED**

Configured shortcuts use the binding name `"open_file"`:

- `src/hotkeys/config.rs:210-215`
- Confirmed by `src/hotkeys/mod.rs:718-739`

**Was broken:** listener recognized `"super::capture_handlers::open_file"` / `"open-file"` but not `"open_file"`.

**Fix:** restored `"open_file" | "open-file"` in `src/daemon/hotkey_listener.rs`. Added `binding_to_daemon_action_maps_open_file_by_name_without_args` so name-only bindings resolve without arg fallback.

### Medium: PR 9 introduced new downstream public API — **FIXED**

**Was broken:** `find_physical_input_device` was public and publicly re-exported.

**Fix:** function and re-export are `pub(crate)` again (`src/daemon/audio.rs`, `src/daemon/mod.rs`). Downstream path `apexshot::daemon::find_physical_input_device` is no longer public API.

### Medium: PR 6 did not complete test ownership — **FIXED**

The audit says recording tests should move when their target functions move. The initial split left them centralized in the parent and imported child internals wholesale.

- **Fix:** moved 10 backend tests to `src/recording/backend.rs:1740-1930` and 10 request/control tests to `src/recording/controls.rs:589-1030`.
- The parent now keeps only four coordinator/output-path tests in `src/recording/mod.rs:403-483`.
- Removed the parent module's broad `backend::*`, `controls::*`, and capture-request imports.

The implementation helpers that were sibling-visible mainly for parent tests are now private to their owning modules, including:

- `src/recording/backend.rs:10` - `CropMargins`
- `src/recording/backend.rs:122` - `compute_wayland_crop`
- `src/recording/backend.rs:234` / `242` - `EncoderProfile` / `PROFILES`
- `src/recording/backend.rs:351` - `ffmpeg_available_encoders`
- `src/recording/backend.rs:984` - `video_encoder_props`
- `src/recording/backend.rs:1017` - `normalize_recording_config_for_profile`
- `src/recording/controls.rs:24` - `should_use_shell_mask_for_request`

The public recording facade and required sibling backend entry points remain unchanged. Targeted recording tests pass: 60 passed.

### Medium: the documented full-test baseline is stale — **FIXED**

| Test | Was | Now |
|------|-----|-----|
| `packaged_desktop_identity_matches_primary_ui_application_id` | searched `src/main.rs` | inspects `src/cli/install.rs` |
| `opensuse_rpm_spec_matches_project_packaging_contract` | searched `src/main.rs` | inspects `src/cli/install.rs` |
| `deb_package_includes_capture_helper_binary` | expected Ubuntu 25.10 | expects Ubuntu 24.04 (matches workflow) |
| `opensuse_installer_contains_reported_dependency_set` | expected `opensuse-install.sh` dispatch | matches current zypper detect + local RPM build messaging |

All four contract tests pass.

### Medium: PR guardrails cannot be validated from commit history — **OPEN**

The refactor exists as a single dirty working tree on top of commit `63be852`. The new split files are untracked while the old monolith files appear deleted or modified. Packaging, workflow, installer, documentation, editor, recording, hotkey, and daemon changes are present together.

As a result, these process claims cannot be proven from repository history:

- One behavior domain per PR.
- First commit moves code without changing behavior.
- Packaging and editor/recording changes are isolated.
- Every conceptual PR ran its Flatpak gate.

The combined snapshot can be validated, but the proposed per-PR review sequence cannot.

### Low: PR 0 formatting / Clippy / Flatpak — **FIXED**

- [x] `cargo fmt --all -- --check` passes (was already green at validation time).
- [x] Three Clippy `needless_return` findings in `src/ocr/mod.rs` fixed.
- [x] Tesseract-only helpers gated with `cfg(feature = "tesseract-ocr")` / `cfg(any(test, feature = "tesseract-ocr"))`; Flatpak feature build is warning-clean aside from the intentional Qt skip note.
- [x] Contract tests green (see above).

### Low: some status descriptions are imprecise — **FIXED**

- The status now identifies the render `pub use` facade as crate-internal because `render` is private in `src/capture/editor.rs:10`.
- The status now assigns countdown, scroll, crosshair, and window-picker drawing to `mode_overlays.rs`, while general selection remains in `drawing/mod.rs`.
- `EDITOR_CSS` is private at `src/capture/editor/ui_support.rs:136`; its nested tests retain access without crate-wide visibility.

---

## Confirmed correct work

### PR 1: dead-code cleanup

- `src/settings/storage.rs` is deleted and no module declaration references it.
- All confirmed internal deletion candidates from the audit are absent.
- The five definition-only `EditorState` methods were removed.
- `cancel_text_edit` was correctly retained and remains called by the Escape handler.
- `get_text_bounds`, `update_text_action`, and `end_select_drag` are gated with `#[cfg(test)]`.
- The listed production-live editor text methods remain present and called.
- Public `ViewTransform::fit` and `ViewTransform::image_to_view` remain unchanged pending an API-policy decision.

### PRs 2-3: payload extraction

- Settings CSS is loaded from `src/settings/settings.css` through `include_str!("settings.css")`.
- Editor CSS is loaded from `src/capture/editor/editor.css` through `include_str!("editor.css")`.
- Byte comparison against the original embedded payloads found both CSS moves unchanged.
- CSS-focused tests inspect the CSS constants rather than Rust source.
- Source-structure tests that still inspect `ui_support.rs` do so intentionally.
- Editor root tests are wired through `#[cfg(test)]` and `#[path = "editor/tests.rs"]` in `src/capture/editor.rs:28-30`.
- A normalized comparison found the moved editor test bodies unchanged.

### PR 4: render split

- `src/capture/editor/render/mod.rs` declares and re-exports `arrows`, `text`, and `effects` correctly.
- Arrow, text, and effect ownership matches the audit's intended seams.
- Callers continue through the existing internal render facade.
- Shared and effect-specific tests remain present.
- Comparison with the pre-split file found all 68 common functions unchanged. The two removed functions are exactly the intentional PR 1 deletions.

### PR 5: overlay drawing split

- The three child modules are wired correctly.
- Recording UI, settings UI, and specialized mode overlays have coherent ownership.
- Dependency flow is one-way; no sibling module cycle was found.
- Comparison with the pre-split file found all 25 functions unchanged.
- Two helpers were safely narrowed from `pub(crate)` to `pub(super)` because no outside caller exists.

### PR 6: recording runtime split

- `audio`, `wf_recorder`, `backend`, and `controls` are wired correctly.
- The dependency direction is coherent: controls use backend/wf-recorder; backend uses wf-recorder/audio; wf-recorder uses audio.
- Existing facade paths for audio listing, control runners, request persistence, and `PreparedOverlayRecordingRequest` remain available.
- Comparison with the pre-split file found all 105 common function bodies unchanged.
- The five old-only functions are exactly the intentional PR 1 dead-code deletions.
- No dependency was added for the split.
- Backend and controls implementation tests are now child-owned, while four coordinator/output-path tests remain in the parent.
- Crop, encoder-profile, normalization, and shell-mask test helpers are private to their owning child modules.

### PR 7: CLI split

- `src/main.rs` is now primarily dispatch, daemon GTK bridging, usage, hotkey command handling, and tests.
- Install, native-host, and capture/OCR/record handlers are in the documented child files.
- `mod cli` exists only in the binary root, so the split did not create library API.
- `--help` and `--version` run successfully.
- **Fixed:** source-inspection contract tests now target `src/cli/install.rs`.

### PR 8: hotkeys split

- Config types, persistence, and accelerator conversion are in `src/hotkeys/config.rs`.
- GNOME, portal, KDE, and wlroots responsibilities are in the documented platform files.
- Existing externally visible names and signatures are preserved.
- The GNOME/portal relationship is one-way and does not form a module cycle.

### PR 9: daemon structure

- Audio monitoring, hotkey listeners, capture handlers, recording handlers, and scroll injection are in the documented child files.
- `run_daemon_inner` remains in `src/daemon/mod.rs` as a real coordinator rather than being moved only for line count.
- Most daemon facade paths remain stable, including clipboard/open helpers, recording notifications, action types, GTK work types, and D-Bus constants.
- No module cycle or unresolved Rust caller was found.
- **Fixed:** stable IPC action/member strings, hotkey name match, and crate-private `find_physical_input_device`.

### Deferred work and metrics

- All 161 `src/**/*.rs` files in the 2026-08-05 validation snapshot were reachable from a crate root; no orphan Rust module was found.
- `capture_overlay.rs` is unchanged from the baseline and remains intentionally deferred.
- PR 10+ started after this validation snapshot. Initial child trees and low-risk slices are present, while the three large GTK coordinators remain and are covered by the implementation plan above.
- No new Cargo dependency was introduced by the structural splits. The current `Cargo.toml` change is a license-expression change.

---

## Validation-snapshot inventory

`src/**/*.rs` inventory at the end of the 2026-08-05 validation/correction pass, before the current PR 10 slices:

| Tier | Files | Lines |
|------|------:|------:|
| XL, at least 2,000 | 6 | 18,606 |
| L, 1,000-1,999 | 12 | 15,817 |
| M, 500-999 | 36 | 26,698 |
| S, below 500 | 107 | 22,435 |
| **Total** | **161** | **83,556** |

Files at or above 2,000 lines in that snapshot:

| File | Exact lines | Assessment |
|------|------------:|------------|
| `src/capture/editor/window/mod.rs` | 4,247 | PR 10+ setup coordinator remains. |
| `src/capture/editor/window/events.rs` | 3,720 | PR 10+ event coordinator remains. |
| `src/capture/editor/state.rs` | 3,678 | State behavior split remains. |
| `src/overlay/window.rs` | 2,483 | PR 10+ overlay coordinator remains. |
| `src/capture_overlay.rs` | 2,431 | Explicitly deferred. |
| `src/capture/editor/tests.rs` | 2,047 | Test payload only. |

This table is retained as validation evidence, not as the live PR 10 inventory. The planning baseline above records the current coordinator paths and approximate spans.

---

## Verification evidence

### Original validation (2026-08-04)

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo check --all-targets` | Pass |
| `cargo check --all-targets --no-default-features --features flatpak` | Pass with 10 unique Tesseract-only dead-code warnings |
| `cargo clippy --all-targets -- -D warnings` | Fail on 3 `needless_return` findings in `src/ocr/mod.rs` |
| `cargo test --lib` | Pass: 584 passed |
| `cargo test --bin apexshot` | Pass: 8 passed |
| `cargo test --test desktop_identity` | Fail: 0 passed, 1 failed |
| `cargo test --test package_metadata` | Fail: 3 passed, 3 failed |
| `git diff --check` | Pass |

### After corrections (2026-08-05)

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo check --all-targets` | Pass |
| `cargo check --all-targets --no-default-features --features flatpak` | Pass (no dead-code warnings) |
| `cargo clippy --all-targets -- -D warnings` | Pass |
| `cargo test --lib recording::` | Pass: 60 passed |
| `cargo test --lib` | Pass: 587 passed |
| `cargo test --bin apexshot` | Pass: 8 passed |
| `cargo test --test desktop_identity` | Pass: 1 passed |
| `cargo test --test package_metadata` | Pass: 6 passed |
| `git diff --check` | Pass |

No live GTK, daemon D-Bus, portal, X11/Wayland capture, audio, or recording smoke test was performed. The manual exit gates in the audit therefore remain unverified.

---

## Recommended correction order

1. [x] Restore the stable daemon strings for `open_file`, the preview D-Bus member, and hotkey name matching. Add focused protocol tests before doing more module movement.
2. [x] Narrow `find_physical_input_device` back to crate-private visibility and re-check the public facade diff.
3. [x] Update the desktop-identity and openSUSE package contract tests to inspect `src/cli/install.rs` after the PR 7 move.
4. [x] Move PR 6 implementation tests to their owning child modules and narrow test-driven visibility.
5. [x] Finish the remaining PR 0 OCR Clippy and Flatpak feature-gating work.
6. [x] Resolve or explicitly baseline the two packaging/workflow failures in their separate workstream. *(tests aligned to current Ubuntu 24.04 workflow and openSUSE installer messaging)*
7. [x] Re-run the full automated gates. [ ] Still needed: the audit's manual smoke matrix before calling PRs 1-9 merge-ready.

All actionable automated/code/documentation corrections are addressed. Remaining before merge-ready: the manual smoke matrix and repository-history evidence for per-PR process guardrails.
