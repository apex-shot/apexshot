# Portal Capture Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make screenshot and recording portal calls use the main ApexShot desktop identity without changing daemon hotkey/autostart identity.

**Architecture:** Add a small RAII-style environment guard that temporarily sets the main desktop identity for portal capture operations and restores previous values afterward. Use that guard only in Wayland screenshot/screencast capture paths and the recording screencast path.

**Tech Stack:** Rust, ashpd, std::env, cargo test

---

### Task 1: Add a temporary portal-capture identity guard

**Files:**
- Modify: `src/utils/mod.rs`
- Create: `src/utils/desktop_env.rs`
- Test: `src/utils/desktop_env.rs`

- [ ] Step 1: Write failing tests for temporary override and restoration.
- [ ] Step 2: Run targeted tests and verify they fail.
- [ ] Step 3: Implement the minimal guard.
- [ ] Step 4: Run targeted tests and verify they pass.

### Task 2: Apply the guard to portal-driven screenshot and screencast capture

**Files:**
- Modify: `src/backend/wayland.rs`
- Test: existing unit test suite for touched modules

- [ ] Step 1: Add the guard to screenshot portal requests and screencast capture requests.
- [ ] Step 2: Run targeted tests.

### Task 3: Apply the guard to recording screencast requests

**Files:**
- Modify: `src/recording/mod.rs`
- Test: existing unit test suite for touched modules

- [ ] Step 1: Add the guard around the recording screencast portal flow.
- [ ] Step 2: Run targeted tests.

### Task 4: Verify no daemon hotkey behavior changed

**Files:**
- Modify: none unless tests need extension
- Test: `src/daemon/mod.rs` existing tests, `tests/desktop_identity.rs`

- [ ] Step 1: Run targeted daemon/desktop identity tests.
- [ ] Step 2: Run a focused cargo test command for all touched modules.
