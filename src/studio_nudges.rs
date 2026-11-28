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

use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Quick Access Copy counts that may reveal the Cloud nudge: the 5th, 15th
/// and 40th lifetime Copy click.
const CLOUD_NUDGE_COPY_SLOTS: [u64; 3] = [5, 15, 40];
/// Dismissals after which the nudge stays quiet for good.
const CLOUD_NUDGE_MAX_DISMISSALS: u64 = 3;
/// Lifetime shows after which the nudge stays quiet for good.
const CLOUD_NUDGE_MAX_SHOWS: u64 = 3;
/// First lifetime screenshot count that can see the nudge.
const CLOUD_NUDGE_MIN_SCREENSHOTS: u64 = 5;

/// Counters used by the local Studio nudges.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct NudgeState {
    screenshots: u64,
    editor_opens: u64,
    copies: u64,
    cloud_nudge_shows: u64,
    cloud_nudge_dismissals: u64,
    cloud_nudge_last_day: String,
    /// The one-time hint that says a capture was named from what it showed.
    name_hint_shown: bool,
    /// A rename happened and the hint still has to appear on a card.
    name_hint_pending: bool,
}

/// Read-only snapshot of the Cloud nudge counters, for `cloud_nudge_due`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudNudgeState {
    pub copies: u64,
    pub shows: u64,
    pub dismissals: u64,
    pub last_day: String,
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

/// Note that the organizer renamed a capture, so the next Quick Access card
/// with room can explain automatic names. A storage failure and a hinted
/// machine both leave the file as it is.
pub fn mark_name_hint_pending() {
    let Some(path) = nudges_path() else {
        return;
    };
    mark_name_hint_pending_at(&path);
}

/// Claim the one-time name hint. Returns true once per machine, and never
/// after the hint has already appeared.
pub fn take_name_hint() -> bool {
    let Some(path) = nudges_path() else {
        return false;
    };
    take_name_hint_at(&path)
}

fn mark_name_hint_pending_at(path: &Path) {
    let _guard = lock_state();

    let mut state = load_state(path);
    if state.name_hint_shown || state.name_hint_pending {
        return;
    }
    state.name_hint_pending = true;
    let _ = save_state(path, &state);
}

fn take_name_hint_at(path: &Path) -> bool {
    let _guard = lock_state();

    let mut state = load_state(path);
    if !state.name_hint_pending || state.name_hint_shown {
        return false;
    }
    state.name_hint_shown = true;
    state.name_hint_pending = false;
    let _ = save_state(path, &state);
    true
}

/// Persist one new Quick Access Copy count and return it. Counts every
/// successful Copy click; a storage failure returns 0 so no Cloud nudge is
/// shown from a counter that could not be recorded.
pub fn bump_copy() -> u64 {
    let Some(path) = nudges_path() else {
        return 0;
    };
    bump_at_path(&path, Counter::Copies)
}

/// The persisted Cloud nudge counters. A missing or unreadable file reads as
/// zero, which keeps the nudge quiet instead of blocking the Copy path.
pub fn cloud_nudge_state() -> CloudNudgeState {
    let Some(path) = nudges_path() else {
        return CloudNudgeState::default();
    };
    let state = load_state(&path);
    CloudNudgeState {
        copies: state.copies,
        shows: state.cloud_nudge_shows,
        dismissals: state.cloud_nudge_dismissals,
        last_day: state.cloud_nudge_last_day,
    }
}

/// Record that the Cloud nudge was revealed on `day` (`YYYY-MM-DD`, local).
pub fn mark_cloud_nudge_shown(day: &str) {
    update_state(|state| {
        state.cloud_nudge_shows = state.cloud_nudge_shows.saturating_add(1);
        state.cloud_nudge_last_day = day.to_string();
    });
}

/// Record one dismissal of the Cloud nudge.
pub fn mark_cloud_nudge_dismissed() {
    update_state(|state| {
        state.cloud_nudge_dismissals = state.cloud_nudge_dismissals.saturating_add(1);
    });
}

/// Local calendar day (`YYYY-MM-DD`) used to show the nudge at most once a day.
pub fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Whether a successful Quick Access Copy click should reveal the Cloud nudge.
///
/// Pure apart from the manual-QA env override: `screenshots` is the lifetime
/// Quick Access preview count and `today` is `today_local()`.
///
/// `APEXSHOT_FORCE_CLOUD_NUDGE=1` is a manual-QA switch: while the destination
/// is ApexShot Cloud and the person is signed out, it ignores the screenshot
/// and Copy counters, the daily cap and the show/dismissal limits. The
/// Settings toggle still applies, so the toggle itself stays testable.
pub fn cloud_nudge_due(
    state: &CloudNudgeState,
    config: &AppConfig,
    today: &str,
    screenshots: u64,
) -> bool {
    if crate::cloud::destination::Destination::from_config(config)
        != crate::cloud::destination::Destination::ApexShot
    {
        return false;
    }
    if !crate::cloud::sign_in::needs_apexshot_sign_in(config) {
        return false;
    }
    if !config.cloud_nudge_enabled {
        return false;
    }
    if cloud_nudge_forced() {
        return true;
    }

    screenshots >= CLOUD_NUDGE_MIN_SCREENSHOTS
        && CLOUD_NUDGE_COPY_SLOTS.contains(&state.copies)
        && state.last_day != today
        && state.dismissals < CLOUD_NUDGE_MAX_DISMISSALS
        && state.shows < CLOUD_NUDGE_MAX_SHOWS
}

fn cloud_nudge_forced() -> bool {
    std::env::var("APEXSHOT_FORCE_CLOUD_NUDGE")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

#[derive(Clone, Copy)]
enum Counter {
    Screenshots,
    EditorOpens,
    Copies,
}

fn bump_at_path(path: &Path, counter: Counter) -> u64 {
    // The capture and editor paths can run in different processes and in
    // several threads of one process; serialize the read-modify-write so two
    // near-simultaneous bumps cannot drop one.
    let _guard = lock_state();

    let mut state = load_state(path);
    let value = {
        let slot = match counter {
            Counter::Screenshots => &mut state.screenshots,
            Counter::EditorOpens => &mut state.editor_opens,
            Counter::Copies => &mut state.copies,
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

/// Read-modify-write the nudge state for a non-counter change (show, dismiss).
/// A storage failure leaves the file alone; the next call behaves as before.
fn update_state(update: impl FnOnce(&mut NudgeState)) {
    let Some(path) = nudges_path() else {
        return;
    };
    let _guard = lock_state();

    let mut state = load_state(&path);
    update(&mut state);
    let _ = save_state(&path, &state);
}

fn lock_state() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
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

    #[test]
    fn name_hint_is_taken_exactly_once() {
        let path = scratch_path("name-hint");
        let _ = fs::remove_dir_all(path.parent().unwrap());

        assert!(!take_name_hint_at(&path), "nothing pending yet");
        mark_name_hint_pending_at(&path);
        assert!(take_name_hint_at(&path));
        assert!(!take_name_hint_at(&path));

        let state = load_state(&path);
        assert!(state.name_hint_shown);
        assert!(!state.name_hint_pending);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_shown_name_hint_is_never_marked_pending_again() {
        let path = scratch_path("name-hint-shown");
        let _ = fs::remove_dir_all(path.parent().unwrap());

        mark_name_hint_pending_at(&path);
        assert!(take_name_hint_at(&path));
        mark_name_hint_pending_at(&path);
        assert!(!take_name_hint_at(&path));
        assert!(!load_state(&path).name_hint_pending);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn state_without_name_hint_keys_reads_as_unshown() {
        let path = scratch_path("name-hint-legacy");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, br#"{"screenshots":3,"copies":1}"#).unwrap();

        let state = load_state(&path);
        assert!(!state.name_hint_shown);
        assert!(!state.name_hint_pending);
        assert!(!take_name_hint_at(&path));

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    fn nudge_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        std::env::remove_var("APEXSHOT_FORCE_CLOUD_NUDGE");
        guard
    }

    fn state_with_copies(copies: u64) -> CloudNudgeState {
        CloudNudgeState {
            copies,
            ..CloudNudgeState::default()
        }
    }

    fn due(copies: u64, screenshots: u64) -> bool {
        let _guard = nudge_env_lock();
        cloud_nudge_due(
            &state_with_copies(copies),
            &AppConfig::default(),
            "2026-11-26",
            screenshots,
        )
    }

    #[test]
    fn nudge_lands_only_on_the_fifth_fifteenth_and_fortieth_copy() {
        for copies in [5, 15, 40] {
            assert!(due(copies, 5), "copy {copies}");
        }
        for copies in [0, 1, 4, 6, 14, 16, 39, 41, 100] {
            assert!(!due(copies, 5), "copy {copies}");
        }
    }

    #[test]
    fn nudge_needs_five_lifetime_screenshots() {
        assert!(!due(5, 4));
        assert!(due(5, 5));
    }

    #[test]
    fn nudge_stays_quiet_when_signed_in() {
        let _guard = nudge_env_lock();
        let config = AppConfig {
            cloud_api_token: "token".into(),
            ..AppConfig::default()
        };
        assert!(!cloud_nudge_due(
            &state_with_copies(5),
            &config,
            "2026-11-26",
            10
        ));
    }

    #[test]
    fn nudge_stays_quiet_for_xbackbone() {
        let _guard = nudge_env_lock();
        let config = AppConfig {
            cloud_destination: "xbackbone".into(),
            ..AppConfig::default()
        };
        assert!(!cloud_nudge_due(
            &state_with_copies(5),
            &config,
            "2026-11-26",
            10
        ));
    }

    #[test]
    fn nudge_respects_the_settings_toggle() {
        let _guard = nudge_env_lock();
        let config = AppConfig {
            cloud_nudge_enabled: false,
            ..AppConfig::default()
        };
        assert!(!cloud_nudge_due(
            &state_with_copies(5),
            &config,
            "2026-11-26",
            10
        ));
    }

    #[test]
    fn nudge_is_shown_at_most_once_a_day() {
        let _guard = nudge_env_lock();
        let state = CloudNudgeState {
            last_day: "2026-11-26".into(),
            ..state_with_copies(5)
        };
        assert!(!cloud_nudge_due(
            &state,
            &AppConfig::default(),
            "2026-11-26",
            10
        ));
        assert!(cloud_nudge_due(
            &state,
            &AppConfig::default(),
            "2026-11-27",
            10
        ));
    }

    #[test]
    fn three_dismissals_silence_the_nudge() {
        let _guard = nudge_env_lock();
        for dismissals in [0, 1, 2] {
            let state = CloudNudgeState {
                dismissals,
                ..state_with_copies(5)
            };
            assert!(
                cloud_nudge_due(&state, &AppConfig::default(), "2026-11-26", 10),
                "dismissals {dismissals}"
            );
        }
        let state = CloudNudgeState {
            dismissals: 3,
            ..state_with_copies(5)
        };
        assert!(!cloud_nudge_due(
            &state,
            &AppConfig::default(),
            "2026-11-26",
            10
        ));
    }

    #[test]
    fn three_lifetime_shows_silence_the_nudge() {
        let _guard = nudge_env_lock();
        let state = CloudNudgeState {
            shows: 3,
            ..state_with_copies(5)
        };
        assert!(!cloud_nudge_due(
            &state,
            &AppConfig::default(),
            "2026-11-26",
            10
        ));
    }

    #[test]
    fn force_env_shows_on_every_copy_while_signed_out() {
        let _guard = nudge_env_lock();
        std::env::set_var("APEXSHOT_FORCE_CLOUD_NUDGE", "1");

        let state = CloudNudgeState {
            copies: 1,
            shows: 9,
            dismissals: 9,
            last_day: "2026-11-26".into(),
        };
        assert!(cloud_nudge_due(
            &state,
            &AppConfig::default(),
            "2026-11-26",
            0
        ));

        let signed_in = AppConfig {
            cloud_api_token: "token".into(),
            ..AppConfig::default()
        };
        assert!(!cloud_nudge_due(&state, &signed_in, "2026-11-26", 0));

        std::env::remove_var("APEXSHOT_FORCE_CLOUD_NUDGE");
    }

    #[test]
    fn shown_and_dismissed_counters_persist_with_the_day() {
        let path = scratch_path("cloud-nudge");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        assert_eq!(bump_at_path(&path, Counter::Copies), 1);

        let mut state = load_state(&path);
        state.cloud_nudge_shows = 1;
        state.cloud_nudge_last_day = "2026-11-26".into();
        assert!(save_state(&path, &state));

        state = load_state(&path);
        state.cloud_nudge_dismissals = 1;
        assert!(save_state(&path, &state));

        let reloaded = load_state(&path);
        assert_eq!(reloaded.copies, 1);
        assert_eq!(reloaded.cloud_nudge_shows, 1);
        assert_eq!(reloaded.cloud_nudge_dismissals, 1);
        assert_eq!(reloaded.cloud_nudge_last_day, "2026-11-26");

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
