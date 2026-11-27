//! Free trial for non-Studio users.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::catalog::{ai_dir, is_installed, tier_from_setting};
use crate::config::AppConfig;

pub const TRIAL_NAMES: u32 = 25;

#[derive(Serialize, Deserialize, Default)]
struct TrialState {
    #[serde(default)]
    trial_used: u32,
    #[serde(default)]
    trial_card_shown: bool,
    #[serde(default)]
    try_card_shown: bool,
}

/// The AI names cards the organizer may show after a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    TrialEnded,
    TryAiNames,
}

impl CardKind {
    /// The value `--ai-names-card` receives.
    pub fn arg(self) -> &'static str {
        match self {
            CardKind::TrialEnded => "trial-ended",
            CardKind::TryAiNames => "try",
        }
    }

    pub fn from_arg(s: &str) -> Option<CardKind> {
        match s {
            "trial-ended" => Some(CardKind::TrialEnded),
            "try" => Some(CardKind::TryAiNames),
            _ => None,
        }
    }
}

pub fn trial_remaining() -> u32 {
    let used = state_path().and_then(|path| read_used(&path)).unwrap_or(0);
    TRIAL_NAMES.saturating_sub(used)
}

pub fn consume_trial() {
    let Some(path) = state_path() else {
        return;
    };
    let used = read_used(&path).unwrap_or(0).saturating_add(1);
    write_used(&path, used);
}

/// Mark a card as shown; returns false if it already was (so it shows once).
pub fn claim_card(kind: CardKind) -> bool {
    let Some(path) = state_path() else {
        return false;
    };
    let mut state = read_state(&path).unwrap_or_default();
    let shown = match kind {
        CardKind::TrialEnded => &mut state.trial_card_shown,
        CardKind::TryAiNames => &mut state.try_card_shown,
    };
    if *shown {
        return false;
    }
    *shown = true;
    write_state(&path, &state);
    true
}

/// AI names may run now: enabled in settings, model installed, and Studio or
/// trial left.
pub fn ai_allowed(config: &AppConfig) -> bool {
    config.organizer_ai_enabled
        && super::hardware::recommend().tier.is_some()
        && is_installed(tier_from_setting(&config.organizer_ai_model))
        && (crate::studio_entitlement::studio_unlocked() || trial_remaining() > 0)
}

fn state_path() -> Option<PathBuf> {
    ai_dir().map(|dir| dir.join("state.json"))
}

fn read_state(path: &Path) -> Option<TrialState> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn read_used(path: &Path) -> Option<u32> {
    read_state(path).map(|state| state.trial_used)
}

fn write_state(path: &Path, state: &TrialState) {
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(raw) = serde_json::to_string(state) else {
        return;
    };
    let mut temp_name = path.as_os_str().to_os_string();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    if fs::write(&temp, raw).is_ok() {
        let _ = fs::rename(&temp, path);
    }
}

fn write_used(path: &Path, used: u32) {
    let mut state = read_state(path).unwrap_or_default();
    state.trial_used = used;
    write_state(path, &state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "apexshot-ai-trial-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or(0)
            ))
            .join("state.json")
    }

    #[test]
    fn the_state_file_round_trips() {
        let path = scratch_path();
        assert_eq!(read_used(&path), None);
        write_used(&path, 3);
        assert_eq!(read_used(&path), Some(3));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn remaining_counts_down_from_the_trial_size() {
        let path = scratch_path();
        write_used(&path, 2);
        let used = read_used(&path).unwrap();
        assert_eq!(TRIAL_NAMES - used, 23);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_card_flags_survive_a_state_round_trip() {
        let path = scratch_path();
        let state = TrialState {
            trial_used: 4,
            trial_card_shown: true,
            try_card_shown: false,
        };
        write_state(&path, &state);
        let loaded = read_state(&path).unwrap();
        assert_eq!(loaded.trial_used, 4);
        assert!(loaded.trial_card_shown);
        assert!(!loaded.try_card_shown);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_old_state_file_loads_without_the_card_flags() {
        let path = scratch_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"trial_used":7}"#).unwrap();
        let state = read_state(&path).unwrap();
        assert_eq!(state.trial_used, 7);
        assert!(!state.trial_card_shown);
        assert!(!state.try_card_shown);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
