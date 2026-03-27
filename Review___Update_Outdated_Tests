I have the following comments after thorough review of file. Implement the comments by following the instructions verbatim.

---
## Comment 1: Arrow hit-testing still follows only the centerline, so large rendered heads and body wings cannot be selected reliably.

In `src/capture/editor/selection.rs`, replace the `AnnotationAction::Arrow` branches in `action_contains_point_with_padding()` and `action_bounds_with_padding()` with logic derived from the real rendered arrow outline for each `ArrowStyle`. Extract or reuse the path/shape geometry from `src/capture/editor/render.rs` so the selectable area includes the actual arrow heads and widened body, not just the centerline. Add regression tests in `src/capture/editor.rs` that click/select on visible head and body edge regions for `Fancy`, `Curved`, and `Double` arrows at larger stroke sizes.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor/selection.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/render.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor.rs
---
## Comment 2: Curved and double arrows lose control-handle editing in Select mode, even though the design treats selection as editable.

Update `src/capture/editor/window/mod.rs` so selected curved/double arrows can render their control handles in Select mode, not only in Arrow mode. Update `src/capture/editor/window/events.rs` to run `arrow_control_handle_at()` and `move_arrow_control_handle()` for selected arrows from the Select-tool interaction path as well. If Select-mode curve editing is intentionally unsupported, then remove the conflicting selection behavior and update the design doc and tests to reflect that narrower contract explicitly.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor/window/mod.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/window/events.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/state.rs
- /home/codegoddy/Desktop/apexshot/docs/superpowers/specs/2026-03-21-arrow-variants-design.md
---
## Comment 3: The unit test suite was only partially updated, leaving obsolete editor fixtures and no regression coverage.

In `src/capture/editor.rs`, update the outdated `AnnotationAction::Text` fixtures and any assertions that still assume the legacy text annotation shape or legacy return signatures. Then add targeted tests for arrow variants that cover `ArrowStyle` persistence, `control_points` initialization, curved/double handle movement, and tool-switch/finalization behavior. Keep those tests aligned with the current contracts defined in `src/capture/editor/types.rs` and `src/capture/editor/state.rs`.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/types.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/state.rs
---