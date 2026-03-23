I have the following comments after thorough review of file. Implement the comments by following the instructions verbatim.

---
## Comment 1: The refreshed tests still omit a dedicated Double-arrow control-handle movement regression, so one requested coverage case remains uncovered.

In `src/capture/editor.rs`, add a dedicated regression test for `ArrowStyle::Double` that selects a double arrow with initialized control points and verifies `move_arrow_control_handle()` updates the start, midpoint, and end behavior as expected. Keep the test on the real `EditorState` path, consistent with the new production-path initialization tests already added nearby.

### Relevant Files
- /home/codegoddy/Desktop/apexshot/src/capture/editor.rs
- /home/codegoddy/Desktop/apexshot/src/capture/editor/state.rs
---