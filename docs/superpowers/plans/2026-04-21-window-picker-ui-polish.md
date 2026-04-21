# Window Picker UI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the GNOME window picker reuse the capture toolbar styling, use a single unambiguous hover target, and replace blue hover accents with the existing warm theme.

**Architecture:** Update the C++ window picker to draw a reduced version of the shared capture toolbar visual language and tighten hover hit testing so only one window tile is active at a time. Add a lightweight source-level regression test that locks the intended toolbar subset and hover behavior.

**Tech Stack:** Qt/C++, Rust tests, CMake

---

### Task 1: Add a failing regression test for the picker UI contract

**Files:**
- Create: `tests/window_picker_ui_contract.rs`
- Test: `tests/window_picker_ui_contract.rs`

- [ ] Write a failing test that checks the picker uses the reduced shared toolbar subset, reverse-order hover hit testing, and no blue hover accents.
- [ ] Run the test and verify it fails.
- [ ] Implement the minimal production changes.
- [ ] Re-run the test and verify it passes.

### Task 2: Reuse capture toolbar styling in the window picker

**Files:**
- Modify: `capture-overlay/src/WindowPickerOverlay.cpp`
- Modify: `capture-overlay/src/WindowPickerOverlay.h`
- Modify: `capture-overlay/src/CaptureOverlay_p.h` only if shared declarations are needed

- [ ] Replace the custom picker toolbar drawing with a reduced shared-style toolbar.
- [ ] Keep only Fullscreen, Window, and Area tools visible in the picker.
- [ ] Match warm hover/active colors to the capture toolbar.

### Task 3: Tighten window hover behavior

**Files:**
- Modify: `capture-overlay/src/WindowPickerOverlay.cpp`

- [ ] Make hover hit testing choose exactly one window card.
- [ ] Iterate hover hits from topmost rendered card backward.
- [ ] Ensure toolbar hover suppresses card hover.

### Task 4: Verify build and focused tests

**Files:**
- Modify: none unless needed

- [ ] Run `cargo test --test window_picker_ui_contract`.
- [ ] Run `cmake --build build/capture-overlay -j`.
- [ ] Summarize manual runtime verification steps.
