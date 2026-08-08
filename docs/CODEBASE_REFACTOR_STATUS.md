# Codebase refactor status

Date: 2026-08-05  
Source plan: [`CODEBASE_REFACTOR_AUDIT.md`](./CODEBASE_REFACTOR_AUDIT.md)  
Scope: `src/` Rust refactor PRs from that audit.

---

## Summary

| Band | Status |
|------|--------|
| **PR 0** (clean merge gates) | **Done** |
| **PRs 1–9** (structural splits) | **Done** |
| **PR 10+** (EditorState / GTK coordinators) | **Started** — first safe slices landed; large coordinators remain |
| **`capture_overlay.rs` split** | **Deferred** (still ~2.4k lines) |

PR 10+ follows the audit rule: extract complete behavior slices with clear owners. Do **not** relocate whole multi-thousand-line functions only to improve line-count metrics.

---

## PR checklist

### PR 0–9 — done

See earlier sections in git history / audit. Short recap:

| PR | Result |
|----|--------|
| 0 | fmt, Clippy, default + Flatpak checks, contract tests |
| 1 | Dead code / orphan module deleted |
| 2–3 | Settings + editor CSS/`include_str!`; editor tests path |
| 4–6 | Render, overlay drawing, recording splits |
| 7–9 | CLI (`cli/`), hotkeys platforms, daemon handlers |

---

### PR 10+: editor state and GTK coordinators — in progress

#### Done (2026-08-05 / 2026-08-06)

##### 1. `EditorState` module tree

`src/capture/editor/state.rs` → `src/capture/editor/state/`:

| File | Role | Approx. lines |
|------|------|--------------:|
| `mod.rs` | `EditorState` / `TextInputState` structs, tool transitions, numbering, and stable facades | ~500 |
| `text_input.rs` | Text input/fit/commit + size/font/re-edit/cancel (**PR 10.3**) | ~850 |
| `crop.rs` | Crop aspect/geometry/drag/apply/fill helpers + `draft_crop_rect` (**PR 10.2**) | ~485 |
| `arrow.rs` | Existing-arrow style/reverse/control-handle edit (**PR 10.1**) | ~190 |
| `history.rs` | Undo/redo/push/history availability | ~130 |
| `drag_draw.rs` | Begin/update/clear/draft/finalize drag + canvas expand (new-arrow construction stays here) | ~440 |
| `export.rs` | `to_rendered_image` / `to_final_image` / background compose; uses `crop::crop_image` | ~250 |
| `effects.rs` | Effect-layer rebuild and action application (**PR 10.6**) | ~40 |

Struct stays in `state/mod.rs` so private field access remains inside the `state` module tree. No new public crate API. `TextInputState` remains in `mod.rs` (sibling event code consumes it).

**PR 10.1 (2026-08-06):** moved arrow style getters/setters, selected-arrow style mutation, reversal, control-handle hit testing/movement, and finalize/cleanup into `state/arrow.rs` with the two arrow unit tests. Method signatures unchanged.

**PR 10.2 (2026-08-06):** moved crop aspect/ratio, selection init/drag/resize/reset/apply, fill/image helpers, and `draft_crop_rect` into `state/crop.rs`. `crop_image` stays module-private to the state tree via the private `crop` module for `export.rs`.

**PR 10.3 (2026-08-06):** consolidated remaining text methods into `state/text_input.rs` (size/font selection, selected-text lookup, commit/re-edit, test-only update, `cancel_text_edit`). Kept `active_text_edit`/`active_text_entry`/Escape cancel path.

**PRs 10.4-10.6 (2026-08-07):** extracted selection state, tool styles, and the optional effects/export helpers while retaining the existing state facades used by editor window code.

##### 2. Editor events — first EventContext-owned families

`src/capture/editor/window/events.rs` → `src/capture/editor/window/events/`:

| File | Role | Approx. lines |
|------|------|--------------:|
| `mod.rs` | `EventContext`, `wire_editor_events` dispatcher, remaining families | ~2,390 |
| `zoom.rs` | `wire_zoom_controls` (zoom UI, ctrl+scroll, pan) | ~260 |
| `history.rs` | `wire_history_buttons` (undo/redo/delete) | ~65 |
| `output.rs` | Copy, upload, Done/save, and traffic-close lifecycle (**PR 10.7**) | ~190 |
| `crop.rs` | Crop apply/reset inspector actions (**PR 10.8**) | ~90 |
| `tools.rs` | Tool-mode button activation and toggle policies (**PR 10.9**) | ~410 |
| `options.rs` | Weight/style/numbering/palette/size option callbacks (**PR 10.10**) | ~685 |

**PR 10.7 (2026-08-07):** moved output lifecycle wiring to `events/output.rs`, preserving save-before-upload, upload suppression, Done/save ordering, preview fallback, and traffic-close behavior. The upload regression contract now inspects this owner.

**PR 10.8 (2026-08-08):** moved crop apply/reset button handlers to `events/crop.rs`. Apply still refreshes canvas content size on success; both actions clear apply-button selection state and update crop size fields. Canvas crop drag remains in `wire_editor_events`.

**PR 10.9 (2026-08-08):** moved Select, Crop, Background, Pen, Arrow, Line, Box, Circle, Text, Number, Obfuscate, Focus, and Highlighter activation to `events/tools.rs`. Crop initialization, effect-rebuild recovery, and toggle policies remain distinct. Identical non-toggling tools share private `wire_standard_tool`.

**PR 10.10 (2026-08-08):** moved weight, style, direction, numbering, palette, size-slider, and obfuscation option callbacks to `events/options.rs` via `ToolOptionsParts`. Text size/font list sync for canvas re-edit remains with click handlers in `mod.rs`.

##### 3. Editor setup — inspector shell + children (**PR 10.11**)

| File | Role | Approx. lines |
|------|------|--------------:|
| `window/inspectors/mod.rs` | Shell facade: tabs, stack, sidebar actions → `InspectorParts` | ~230 |
| `window/inspectors/shell.rs` | Shared panel/section helpers | ~50 |
| `window/inspectors/select.rs` | Select inspector panel | ~40 |
| `window/inspectors/crop.rs` | Crop inspector panel | ~35 |
| `window/inspectors/stroke.rs` | Pen/arrow/line/highlighter panels | ~90 |
| `window/inspectors/text.rs` | Text inspector panel | ~30 |
| `window/inspectors/number.rs` | Number inspector panel | ~40 |
| `window/inspectors/obfuscate.rs` | Obfuscate inspector panel | ~25 |

Same setup-facing entry as before (`build_tool_inspectors` + `InspectorContentInputs`). Child modules own panel assembly with narrower input structs.

**PR 10.12 (2026-08-08):** extracted the async effects pipeline to `window/effects.rs` (`install_async_effects_pipeline` → `rebuild_effects_async`). Channels, worker drain/coalesce, 16ms poll, stale-revision reject, and 500ms/2s watchdog preserved.

**PR 10.13 (2026-08-08):** extracted background asset loading (`window/background_assets.rs`) and canvas layout (`window/canvas_layout.rs`) as separate services. Setup installs each independently; draw still uses returned cache handles + layout callback.

**PR 10.14 (2026-08-08):** extracted window chrome (`window/chrome.rs`) and empty drop zone (`window/empty_state.rs`). Session reuse controller cleanup stays in setup.

##### 4. Overlay window peel

`src/overlay/window.rs` → `src/overlay/window/`:

| File | Role | Approx. lines |
|------|------|--------------:|
| `mod.rs` | `setup_window` + selection result + aspect helpers | ~2,060 |
| `audio.rs` | Local PW audio meters / daemon poll | ~310 |
| `platform.rs` | Overlay CSS + X11 compositor animation suppress | ~120 |

##### 5. Daemon coordinator thin-out

| File | Role | Approx. lines |
|------|------|--------------:|
| `daemon/dispatch.rs` | `dispatch_daemon_action` (full action `match`) | ~220 |
| `daemon/mod.rs` | `run_daemon_inner` setup + loop calling dispatch | ~980 total; **`run_daemon_inner` ~170 lines** |

`run_daemon_inner` is now a real coordinator (handlers already out in PR 9; action match out in PR 10).

Structure tests that `include_str!` production sources were updated to follow moved modules (`inspectors/*`, `events/{mod,zoom,history,output,tools,crop,options}.rs`, `overlay/window/mod.rs`).

---

## Still large — tackle next (tomorrow)

These are the remaining high-coupling coordinators and oversized files. Prefer **behavior slices with clear owners**, not whole-function file moves.

### Logic-bearing coordinators still over ~1,000 lines

| Function | Location | Approx. span | Why it is hard |
|----------|----------|-------------:|----------------|
| **`wire_editor_events`** | `capture/editor/window/events/mod.rs` | **~2,150 lines** | GTK callbacks share `EventContext`, widgets, and sync closures. Zoom, history, output, crop actions, tool-mode, and options are out; canvas drag/click/motion and keyboard remain. |
| **`setup_editor_window_full`** | `capture/editor/window/mod.rs` | **~2,990 lines** | Construction order, widget lifetimes, async effects, `set_draw_func`. Inspector shell is out; draw_func + layout chrome remain. |
| **`overlay::window::setup_window`** | `overlay/window/mod.rs` | **~1,850 lines** | Motion hit-test, toolbar clicks, drag gestures, keyboard. Audio/CSS/X11 helpers are out; input surface remains. |

### Other large files (navigation / deferred)

| File | Approx. lines | Notes |
|------|--------------:|-------|
| `capture/editor/window/mod.rs` | ~3,690 | Setup coordinator + tests; effects/assets/layout extracted |
| `capture/editor/window/events/mod.rs` | ~2,390 | Dominated by `wire_editor_events` (drag/click/keyboard remain) |
| `capture_overlay.rs` | ~2,430 | **Deferred** process-boundary split (audit: after daemon/capture) |
| `capture/editor/state/mod.rs` | ~500 | EditorState struct, tool transitions, numbering, and stable state facades |
| `overlay/window/mod.rs` | ~2,060 | Dominated by `setup_window` |
| `capture/editor/tests.rs` | ~2,050 | Test payload only |
| `recording/backend.rs` | ~1,930 | Already split out of recording facade; further internal split optional |
| `recording/stop_overlay.rs` | ~1,710 | Separate from main overlay window |

### Recommended next extractions (ordered by safety)

1. **Editor events** (validation numbering)
   - [x] PR 10.7 Save / Done / copy / upload button families
   - [x] PR 10.8 Crop apply/reset buttons (`events/crop.rs`)
   - [x] PR 10.9 Tool-button mode switches (`events/tools.rs`)
   - [x] PR 10.10 Tool options (`events/options.rs`)
   - **Defer** canvas drag (~800 lines), canvas click/motion (~770), full keyboard until shared helpers exist

2. **Editor setup**
   - [x] PR 10.11 Inspector children (`window/inspectors/`)
   - [x] PR 10.12 Async effects pipeline (`window/effects.rs`)
   - [x] PR 10.13 Background assets + canvas layout (`background_assets.rs`, `canvas_layout.rs`)
   - [x] PR 10.14 Empty state + chrome (`empty_state.rs`, `chrome.rs`)
   - Crop dim fields + option-list construction (returns widgets) — still in setup
   - **Defer** `set_draw_func` body until Parts ownership is clear (Track D / later)

3. **More `EditorState` child modules** (Track A in validation plan)
   - [x] PR 10.1 arrow editing (`state/arrow.rs`)
   - [x] PR 10.2 crop (`state/crop.rs`)
    - [x] PR 10.3 text consolidation (`state/text_input.rs`)
    - [x] PR 10.4 selection (`state/selection.rs`)
    - [x] PR 10.5 tool-style (`state/tool_style.rs`)
    - [x] PR 10.6 effects/export cleanup (`state/effects.rs`, `state/export.rs`)
   - Keep struct in `state/mod.rs`; child `impl EditorState` only inside the `state` tree

4. **Overlay `setup_window`**
   - Window shell construction (window + drawing area + geometry) → `OverlayWindowParts`
   - **Defer** toolbar-click and motion blocks without shared hit-test ownership

5. **`capture_overlay.rs`**
   - Worker lifecycle, output parsing, wlroots routing, recording request spawn — separate PR after coordinator work settles

6. **Manual smoke** (still required before calling PR 10 “done”)
   - Capture → editor annotate/undo/redo/crop/save  
   - Settings open/save/close  
   - Daemon hotkey + record start/pause/resume/stop  
   - Overlay on X11 and Wayland as applicable  

---

## Success metrics — progress

| Metric | Audit baseline | Target | Now |
|--------|---------------:|-------:|-----|
| Unreachable Rust modules | 1 | 0 | **0** |
| Confirmed caller-free internal functions | 17 | 0 | **Addressed (PR 1)** |
| Flatpak dead-code warnings | 10 | 0 | **0** |
| Logic-bearing coordinators over 1,000 lines | 3 | 0 after GTK work | **3 remain** (`wire_editor_events`, `setup_editor_window_full`, `setup_window`) |
| `run_daemon_inner` as thin coordinator | large match inline | handlers + dispatch out | **Done** (~170 lines) |
| New public API from splits | 0 | 0 | **Held** |

A 3,000-line function moved into a 3,000-line file is **not** success. Line count is secondary to ownership.

---

## Guardrails (still in force)

- One behavior domain per change set  
- First commit moves code; follow-ups may simplify  
- No new dependencies for module splitting  
- No public re-export changes unless reviewed as API  
- No whole-function relocation solely for file-size targets  
- No mass removal of `#[allow(dead_code)]`  
- Keep `cargo check --all-targets --no-default-features --features flatpak` in every gate  

---

## Verification

Re-run before merge / after tomorrow’s work:

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo check --all-targets --no-default-features --features flatpak
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

PR 10 slice automated coverage already exercised:

- `cargo test --lib -- capture::editor`  
- `cargo test --lib -- daemon::`  
- `cargo test --test window_picker_ui_contract`  
