I have the following comments after thorough review of file. Implement the comments by following the instructions verbatim.

---
## Comment 1: Arrow hit-testing still uses a hand-copied polygon, so smooth rendered caps and arcs can remain unselectable.

In `src/capture/editor/render.rs`, extract a shared arrow-outline builder that represents the exact filled shape used by `build_thorn_arrow_path()` and `build_double_arrow_path()`, including the smooth cap/head arcs. Update `src/capture/editor/selection.rs` so `action_bounds_with_padding()` and `action_contains_point_with_padding()` consume that shared geometry instead of maintaining separate outline math. Extend the arrow hit-test assertions in `src/capture/editor.rs` to cover painted rounded regions, not only the simplified polygon corners.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor/selection.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/render.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor.rs
---
## Comment 2: The new arrow tests still miss Curved/Double body-edge cases and reimplement control-point setup instead of exercising production flows.

In `src/capture/editor.rs`, replace the test-local control-point initialization blocks with tests that invoke the real production paths in `EditorState`, such as `finalize_drag_action()` or other state methods that are responsible for creating arrow data. Add explicit hit-test cases for visible body-edge regions on `ArrowStyle::Curved` and `ArrowStyle::Double`, not just head clicks. Keep the assertions aligned with the runtime behavior implemented in `src/capture/editor/state.rs` and `src/capture/editor/selection.rs` rather than duplicating logic from `src/capture/editor/window/events.rs` inside the tests.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/state.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/selection.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/window/events.rs
---