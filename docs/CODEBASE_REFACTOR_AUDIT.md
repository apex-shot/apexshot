# Codebase refactor audit: validated findings and execution plan

Date: 2026-08-04
Scope: `src/` Rust code. Packaging, CI, Flatpak manifests, and the C++ helper are not refactor targets.
Status: validated against the current working tree; no refactors applied.

---

## Executive verdict

The original audit was directionally right, but its plan mixed three different kinds of work:

1. Proven dead code that can be deleted safely inside this repository.
2. Embedded payloads (CSS and tests) that can be moved mechanically.
3. Multi-thousand-line GTK coordinator functions whose extraction changes ownership, captures, visibility, and event wiring.

Those categories need different review and test gates.

Validated conclusions:

- The size inventory is exact: **137 files, 88,655 lines** under `src/`.
- `src/settings/storage.rs` is the only unreachable Rust source file.
- Most listed dead functions really have no in-repository callers.
- The proposed blanket deletion of the old editor text API is **not valid**. Several listed methods are used by production editor event code.
- `setup_editor_window_full`, `wire_editor_events`, and `overlay::window::setup_window` are the riskiest work. Moving each whole function to another file would improve a line-count metric without improving its design.
- The Flatpak feature build exposes a separate issue: Tesseract-only OCR helpers are not feature-gated and produce ten dead-code warnings.

---

## Validation method

1. Walked `mod` declarations from `src/lib.rs`, `src/main.rs`, and `src/bin/*.rs`, including `#[path = ...]` modules.
2. Indexed the current repository and checked inbound call edges for each claimed dead function or method.
3. Cross-checked exact identifier references across `src/**/*.rs` to catch callback and test-only uses.
4. Counted every Rust source line and identified embedded CSS/test ranges separately from production logic.
5. Ran the default build, a dead-code-denied build, the Flatpak feature build, tests, formatting, and Clippy.

Limits:

- In-repository reference checks do not prove that public library API has no downstream users.
- No GUI smoke test was performed during this audit.
- Existing unrelated packaging/workflow changes make two packaging-contract tests fail; details are recorded below.

---

## Scale

| Tier | Files | Lines |
|------|------:|------:|
| XL >=2000 | 14 | 43,255 |
| L 1000-1999 | 8 | 10,669 |
| M 500-999 | 23 | 16,889 |
| S <500 | 92 | 17,842 |
| **Total** | **137** | **88,655** |

The top 15 files contain 44,968 lines, or **50.7%** of `src/`.

File size alone overstates some problems:

| File | Embedded payload | Production implication |
|------|------------------|------------------------|
| `capture/editor.rs` | Tests start at line 28 | The production module root is about 27 lines. |
| `settings/ui_support.rs` | CSS is lines 4-2054 | The Rust helper logic is small. |
| `capture/editor/ui_support.rs` | CSS is inside lines 136-2986 | Extracting the string removes about 2,850 lines without changing behavior. |
| `recording/mod.rs` | Main test module is lines 3318-4021 | Moving tests removes about 700 lines, but the production module remains large. |
| `capture/editor/window/mod.rs` | Tests are lines 3864-4247 | Moving tests helps navigation but leaves the 3,186-line setup function. |

---

## Reachability

The module walk reached 136 of 137 Rust files with no unresolved `mod` declarations.

| File | Result | Action |
|------|--------|--------|
| `src/settings/storage.rs` | Unreachable: never declared by `settings/mod.rs` or another root | Delete. |

No other `.rs` file is an orphan. `src/overlay.rs` plus `src/overlay/` is normal Rust module layout, not duplication.

---

## Dead-code findings

### Confirmed internal deletions

These items have no in-repository caller and are not externally reachable through a public module path.

| Location | Item(s) | Validation |
|----------|---------|------------|
| `settings/actions.rs` | `close_window` | Definition only; current close button calls `window.close()` directly. |
| `settings/windowing.rs` | `SETTINGS_WINDOW_MIN_WIDTH`, `SETTINGS_WINDOW_MIN_HEIGHT` | Definition only. |
| `settings/windowing.rs` | `install_autostart_entry_for_current_exe` | Definition only; `install_autostart_entry_smart` is the live settings path. |
| `recording/mod.rs` | `pipewire_source_pipeline` | Definition only. |
| `recording/mod.rs` | `select_audio_encoder` | Definition only. |
| `recording/mod.rs` | `video_raw_caps` | Definition only; the similarly named local variable at line 1801 is unrelated. |
| `recording/mod.rs` | `video_queue_props` | Definition only. |
| `recording/mod.rs` | `video_post_encoder_caps` | Definition only. |
| `capture/editor/render.rs` | `draw_censor_draft_rect` | Definition only. |
| `capture/editor/render.rs` | `apply_secure_pixelate` | Definition only; live pixelation uses `apply_censor_rect`. |
| `capture/editor/ui_support.rs` | `recommended_window_size` | Definition only; the live caller uses `recommended_window_size_with_extra_width`. |
| `capture/preview_overlay.rs` | `open_target` | Definition-only wrapper around `utils::open::open_path`. |
| `capture/editor/io_ops.rs` | `open_target` | Definition-only wrapper around `utils::open::open_path`. |

Deleting the two `open_target` wrappers is enough. There are no callers to redirect.

### Public API caveat

`capture/editor/types.rs::ViewTransform::image_to_view` is definition-only in this repository, but `capture::editor::types` is public. Deleting it can break downstream library users even though it is unused here.

Treat `image_to_view` and the test-only `ViewTransform::fit` as an API decision, not part of the safe internal deletion PR. First decide whether the crate promises a public Rust API. If it does, deprecate or wait for a breaking release; if it does not, document that policy and then remove/gate them.

### EditorState correction

The original audit's "likely leftover text-input API" group was too broad.

Production-live methods currently hidden by stale `#[allow(dead_code)]` attributes include:

- `start_text_input`
- `add_text_input_char`
- `delete_text_input_char`
- `move_cursor_left` and `move_cursor_right`
- `tick_cursor_blink`
- `commit_text_input` and `commit_active_text_input`
- `cancel_text_input` and its private cleanup helper
- `get_text_input`
- `fit_active_text_to_layout_preserving_font_size`
- `fit_active_text_to_layout_preserving_box`
- `set_text_size`
- `obfuscate_method`

Remove the stale allow attributes from these live items; do not delete them.

Definition-only internal methods that remain valid deletion candidates:

- `fit_active_text_to_layout_preserving_height`
- `set_selected_obfuscate_action_amount` (the live path uses the `without_rebuild` variant)
- `set_active_size` (the live path uses the `without_rebuild` variant)
- `select_text_action_at_point_with_scale`
- `commit_text_edit`

Test-only methods should be `#[cfg(test)]`, not described as production behavior:

- `get_text_bounds`
- `update_text_action`
- `end_select_drag`

`cancel_text_edit` is production-live through the Escape handler in `window/events.rs`; it must not be deleted with `commit_text_edit`. Its two old fields, `active_text_edit` and `active_text_entry`, appear never to hold a value, but remove them only in a focused text-state cleanup with the existing text-input tests running.

### Live items with misleading allows

- `install_autostart_entry_smart` and `uninstall_autostart_entry` are called by settings save.
- `autostart_dir` is used by both live functions.
- `text_detection_ready` and `text_detection_handle` are used by editor window setup; the handle also owns background work.
- `recording::stop_overlay` is re-exported and used by CLI and daemon recording paths.
- Settings widget holder structs are constructed and their fields are consumed across the settings window assembly.

Do not mass-delete items merely because they carry `#[allow(dead_code)]`.

---

## Duplicate paths

Validated duplication:

- `intern_atom` and `send_net_wm_state_client_message` are copied in three places, not two: editor window, preview overlay, and recording stop overlay.
- The preview and recording "always on top" paths are nearly identical; the editor uses the same primitives for add/remove state.
- Settings and editor CSS are live and are good `include_str!` candidates.
- CLI install/uninstall autostart and settings-toggle autostart are different entry points with different responsibilities. Do not merge them during a structural refactor.

The X11 helper consolidation is valid but lower priority than dead deletion and CSS extraction. It should be one focused PR with X11 smoke coverage, not mixed into an editor split.

---

## Oversized code by risk

### Mechanical payload moves

These are the safest size reductions because they do not split runtime ownership:

1. `settings/ui_support.rs`: move `SETTINGS_CSS` to `settings/settings.css` and use `include_str!`.
2. `capture/editor/ui_support.rs`: move the editor CSS string to `capture/editor/editor.css` and use `include_str!`.
3. `capture/editor.rs`: move the test module to `capture/editor/tests.rs` with `#[path = "editor/tests.rs"]`.
4. Move large trailing test modules when their owning production module is split, especially recording, editor window, daemon, hotkeys, and main.

The editor CSS tests currently inspect `include_str!("ui_support.rs")`. CSS assertions must inspect an `EDITOR_CSS` constant after extraction; source-structure assertions can continue to inspect Rust source.

After only the first three moves, XL files fall from 14 to 11. That is useful cleanup, not completion.

### Large files with movable top-level groups

These files contain many standalone functions and can be split one cohesive group at a time while keeping a facade in `mod.rs`:

| File | First useful seams |
|------|--------------------|
| `main.rs` | install/uninstall, native host, capture/OCR, record, daemon launch, usage |
| `recording/mod.rs` | wf-recorder, Wayland/FFmpeg, audio discovery, output paths, control orchestration |
| `hotkeys/mod.rs` | GNOME, portal, KDE, wlroots config emitters, config persistence |
| `capture/editor/render.rs` | arrows, text, effects, selection/crop drawing |
| `overlay/drawing.rs` | recording panel, settings tabs/popups, selection/countdown/window-picker drawing |
| `daemon/mod.rs` | audio monitoring, hotkey listeners, capture handlers, recording handlers, DBus facade |
| `capture_overlay.rs` | worker lifecycle, output parsing, wlroots routing, recording request spawn |

These moves should preserve the current public facade and use only the minimum `pub(super)` or `pub(crate)` visibility required.

### High-coupling coordinator functions

| Function | Span | Static complexity | Why it is not mechanical |
|----------|-----:|------------------:|--------------------------|
| `wire_editor_events` | 3,391 lines | cyclomatic 392, cognitive 961 | GTK callbacks share state, widgets, and local synchronization closures. |
| `setup_editor_window_full` | 3,186 lines | cyclomatic 207, cognitive 450 | Owns construction order, widget lifetimes, callbacks, and session state. |
| `overlay::window::setup_window` | 1,846 lines | cyclomatic 206, cognitive 797 | Combines window policy, input, hit testing, selection, and recording UI. |
| `daemon::run_daemon_inner` | 340 lines | cyclomatic 77, cognitive 247 | Smaller by lines but coordinates DBus, tray, hotkeys, recording, and shutdown. |

Moving any whole function into `setup.rs` or `events/mod.rs` does not reduce complexity. Extract only a complete behavior slice whose inputs already have a clear owner. Do not create a new "god context" merely to make the move compile.

---

## Build validation baseline

| Command | Result |
|---------|--------|
| `cargo check --all-targets` | Pass |
| `RUSTFLAGS='-D dead_code' cargo check --lib --bin apexshot` | Pass; existing allows and public items still suppress/avoid this lint. |
| `cargo check --all-targets --no-default-features --features flatpak` | Pass with 10 dead-code warnings in Tesseract-only OCR helpers. |
| `cargo test --all-targets` | Rust library tests: 584 pass. Main binary tests: 8 pass. Desktop identity: 1 pass. Two packaging-contract tests fail. |
| `cargo fmt --all -- --check` | Fails on existing formatting in `app_identity.rs`, `usage_telemetry.rs`, and `main.rs`. |
| `cargo clippy --all-targets -- -D warnings` | Fails on three existing `needless_return` findings in `ocr/mod.rs`. |

Current packaging-test failures, outside this audit's refactor scope:

- `deb_package_includes_capture_helper_binary`
- `opensuse_installer_contains_reported_dependency_set`

Before enforcing the full test command as a refactor merge gate, finish or separately fix the current packaging/workflow changes. Do not mix those fixes into the `src/` refactor PRs.

---

## Execution plan

### PR 0: establish clean refactor gates

Keep this separate from structural work.

1. Apply the existing rustfmt output.
2. Fix the three OCR Clippy findings without changing behavior.
3. Feature-gate Tesseract-only OCR structs/functions so the Flatpak check is warning-free.
4. Resolve the two packaging-contract failures in the packaging workstream, or record them as an explicit temporary baseline until that work lands.

Exit gate: default check, Flatpak check, format, and Clippy are green; code tests are green.

### PR 1: proven dead code only

1. Delete `src/settings/storage.rs`.
2. Delete the confirmed internal functions/constants in the table above.
3. Delete the two caller-free `open_target` wrappers.
4. Delete only the five definition-only `EditorState` methods listed above.
5. Gate the three state test-only methods with `#[cfg(test)]`.
6. Remove stale `#[allow(dead_code)]` attributes from the adjacent live methods that the compiler proves are used.
7. Leave public `ViewTransform` methods unchanged pending the API-policy decision.

Exit gate: no behavior changes, no new allows, default and Flatpak checks pass, and editor/recording/settings tests pass.

### PR 2: settings CSS extraction

1. Move `SETTINGS_CSS` byte-for-byte to `src/settings/settings.css`.
2. Keep the Rust constant as `include_str!("settings.css")`.
3. Keep the unsupported-property test pointed at the constant.

Exit gate: settings CSS test plus a manual settings-window open/save/close smoke test.

### PR 3: editor CSS and test payload extraction

Use two commits or two PRs if review size matters.

1. Move editor CSS byte-for-byte to `src/capture/editor/editor.css`.
2. Update CSS-focused tests to inspect `EDITOR_CSS` rather than Rust source.
3. Move `capture/editor.rs` tests to `capture/editor/tests.rs` without renaming tests or changing bodies.

Exit gate: all editor tests pass; manual editor open, annotate, undo/redo, crop, and save smoke test.

### PRs 4-6: split low-coupling function families

Do one family per PR, preserving function bodies first and improving code only later.

1. Render: `arrows.rs`, `text.rs`, then `effects.rs` under an unchanged render facade.
2. Overlay drawing: recording UI, settings UI/popups, then mode overlays.
3. Recording: wf-recorder, Wayland/FFmpeg, audio discovery, then control orchestration. Move the trailing recording tests when their target functions move.

Exit gate per PR: no new public API, no new dependency, unchanged tests, and a smoke test only for the affected feature.

### PRs 7-9: split process and platform orchestration

1. `main.rs`: move install/uninstall first, native host second, then capture/OCR and record handlers. Keep `main` and `async_main` as dispatchers.
2. `hotkeys/mod.rs`: move GNOME, portal, KDE, and wlroots-specific code one platform at a time; keep config types and the public facade stable.
3. `daemon/mod.rs`: move audio monitoring, hotkey listeners, capture handlers, and recording handlers before touching `run_daemon_inner`.
4. Split `capture_overlay.rs` only after daemon/capture boundaries settle.

Exit gate: CLI help/version and affected command tests; daemon hotkey and recording start/stop smoke tests for relevant PRs.

### PR 10+: editor state and GTK coordinators

1. Split `EditorState` impls by behavior only after the dead/test-only cleanup. Keep the struct in `state/mod.rs`; use child modules so field access remains local to the state module tree.
2. Extract one event family from `wire_editor_events` only when its dependencies already live in `EventContext` or a smaller existing owner.
3. Extract one construction section from `setup_editor_window_full` only when it returns a concrete widget/result and does not require a new bag of unrelated fields.
4. Apply the same rule to `overlay::window::setup_window`.
5. Refactor `run_daemon_inner` last, after its handlers have moved out and it can become a coordinator naturally.

Exit gate: all automated checks plus full manual capture -> editor -> save, settings save, daemon hotkey, record start/pause/resume/stop, and X11/Wayland smoke coverage as applicable.

---

## Guardrails

- One behavior domain per PR.
- First commit moves code; follow-up commits may simplify it.
- No new dependencies for module splitting.
- No public re-export changes unless explicitly reviewed as API changes.
- No whole-function relocation solely to satisfy a file-size target.
- No mass removal of `#[allow(dead_code)]`; remove each allow when its item is proven live, deleted, feature-gated, or test-gated.
- Do not combine editor, recording, and packaging changes.
- Keep `cargo check --all-targets --no-default-features --features flatpak` in every PR gate.

---

## Success metrics

Primary metrics:

| Metric | Current | Target |
|--------|--------:|-------:|
| Unreachable Rust modules | 1 | 0 |
| Confirmed caller-free internal functions in this audit | 17 | 0 |
| Flatpak dead-code warnings | 10 | 0 |
| Logic-bearing coordinator functions over 1,000 lines | 3 | 0, after GTK coordinator work |
| New public API introduced by splits | 0 | 0 |

Secondary navigation metrics:

| Metric | Current | Near-term target |
|--------|--------:|-----------------:|
| Files >=2000 lines | 14 | 11 after payload extraction; <=4 after low-coupling splits |
| Largest production coordinator | 3,391 lines | <500 only when behavior slices have real ownership |

The line-count target is secondary. A 3,000-line function moved into a 3,000-line file is not a successful refactor.

---

## Recommended starting point

Start with PR 0 only if clean merge gates are required immediately. Otherwise, PR 1 is the smallest useful refactor: delete the orphan and proven internal dead code while explicitly leaving public API and live editor text behavior alone.
