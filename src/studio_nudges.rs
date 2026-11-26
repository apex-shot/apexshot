//! Local, persisted counters for the Studio nudges.
//!
//! These counters are deliberately separate from usage telemetry: they drive
//! the "Try a styled look" chip and the occasional editor hint even when the
//! user has turned telemetry off. They carry no identity and never leave the
//! machine.
//!
//! Storage is a tiny JSON file under the same XDG cache directory as the usage
//! counters. Every failure is open: a missing, unreadable or unwritable file
//! behaves like a fresh install (count 0) instead of blocking the capture or
//! editor path.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Counters used by the local Studio nudges.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
struct NudgeState {
    screenshots: u64,
    editor_opens: u64,
}

fn nudges_path() -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push("studio_nudges.json");
    Some(path)
}

fn load_state(path: &Path) -> NudgeState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &NudgeState) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec(state) else {
        return false;
    };
    fs::write(path, bytes).is_ok()
}

/// Persist one new screenshot count and return it. Counts one Quick Access
/// preview per call; a storage failure returns 0 so callers behave as if this
/// were the first capture.
pub fn bump_screenshot() -> u64 {
    let Some(path) = nudges_path() else {
        return 0;
    };
    bump_at_path(&path, Counter::Screenshots)
}

/// Persist one new editor-open count and return it. Counts one image editor
/// window opened with an image; a storage failure returns 0 so the local
/// nudges still appear on a machine that cannot write the file.
pub fn bump_editor_open() -> u64 {
    let Some(path) = nudges_path() else {
        return 0;
    };
    bump_at_path(&path, Counter::EditorOpens)
}

/// Whether the lifetime screenshot count is one of the quiet hint slots.
pub fn should_show_editor_hint(screenshot_count: u64) -> bool {
    matches!(screenshot_count, 10 | 30 | 60)
}

#[derive(Clone, Copy)]
enum Counter {
    Screenshots,
    EditorOpens,
}

fn bump_at_path(path: &Path, counter: Counter) -> u64 {
    // The capture and editor paths can run in different processes and in
    // several threads of one process; serialize the read-modify-write so two
    // near-simultaneous bumps cannot drop one.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());

    let mut state = load_state(path);
    let value = {
        let slot = match counter {
            Counter::Screenshots => &mut state.screenshots,
            Counter::EditorOpens => &mut state.editor_opens,
        };
        *slot = slot.saturating_add(1);
        *slot
    };
    if save_state(path, &state) {
        value
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_path(case: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "apexshot-studio-nudges-test-{}-{case}",
                std::process::id()
            ))
            .join("studio_nudges.json")
    }

    #[test]
    fn counters_persist_and_increment_independently() {
        let path = scratch_path("counters");
        let _ = fs::remove_dir_all(path.parent().unwrap());

        assert_eq!(bump_at_path(&path, Counter::Screenshots), 1);
        assert_eq!(bump_at_path(&path, Counter::Screenshots), 2);
        assert_eq!(bump_at_path(&path, Counter::EditorOpens), 1);
        assert_eq!(bump_at_path(&path, Counter::EditorOpens), 2);

        let reloaded = load_state(&path);
        assert_eq!(reloaded.screenshots, 2);
        assert_eq!(reloaded.editor_opens, 2);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_or_corrupt_file_starts_at_zero() {
        let path = scratch_path("corrupt");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(load_state(&path).screenshots, 0);
        assert_eq!(load_state(&path).editor_opens, 0);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        assert_eq!(bump_at_path(&path, Counter::EditorOpens), 1);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn hints_land_on_the_tenth_thirtieth_and_sixtieth_screenshot_only() {
        for count in [10, 30, 60] {
            assert!(should_show_editor_hint(count), "{count}");
        }
        for count in [0, 1, 9, 11, 29, 31, 59, 61, 100] {
            assert!(!should_show_editor_hint(count), "{count}");
        }
    }
}
