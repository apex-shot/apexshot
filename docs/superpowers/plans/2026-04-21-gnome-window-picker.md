# GNOME Window Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the GNOME Wayland custom window picker show currently visible windows with static thumbnails from the GNOME Shell extension.

**Architecture:** Extend the existing GNOME Shell extension with a new `org.apexshot.WindowList` D-Bus service that enumerates visible normal app windows and captures a static thumbnail for a selected window. Keep the C++ picker UI as the presentation layer and feed it data from the extension.

**Tech Stack:** GNOME Shell extension (GJS), D-Bus, Qt/C++, cargo test

---

### Task 1: Add a failing extension-side contract test

**Files:**
- Modify: `gnome-extension/extension.js`

- [ ] Write a failing test/helper assertion for visible-window filtering and JSON shape.
- [ ] Run the relevant test/validation command if available, otherwise use syntax validation.
- [ ] Implement the minimal window-list helpers.
- [ ] Re-run validation.

### Task 2: Export the WindowList D-Bus API from the GNOME extension

**Files:**
- Modify: `gnome-extension/extension.js`

- [ ] Add introspection XML for `org.apexshot.WindowList`.
- [ ] Export `GetWindows` and `CaptureWindowById`.
- [ ] Wire enable/disable lifecycle cleanup.

### Task 3: Make the C++ picker consume extension-provided windows cleanly

**Files:**
- Modify: `capture-overlay/src/WindowPickerOverlay.cpp`

- [ ] Tighten client-side handling for missing thumbnails and invalid entries.
- [ ] Keep static thumbnail fallback via `CaptureWindowById`.
- [ ] Build/validate the picker path.

### Task 4: Verify end-to-end integration remains green

**Files:**
- Modify: none unless validation requires fixes

- [ ] Run Rust library tests.
- [ ] Run a GNOME extension syntax check command.
- [ ] Summarize manual runtime verification steps.
