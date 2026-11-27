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

/// AI names may run now: enabled in settings, model installed, and Studio or
/// trial left.
pub fn ai_allowed(config: &AppConfig) -> bool {
    config.organizer_ai_enabled
        && is_installed(tier_from_setting(&config.organizer_ai_model))
        && (crate::studio_entitlement::studio_unlocked() || trial_remaining() > 0)
}

fn state_path() -> Option<PathBuf> {
    ai_dir().map(|dir| dir.join("state.json"))
}

fn read_used(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    let state: TrialState = serde_json::from_str(&raw).ok()?;
    Some(state.trial_used)
}

fn write_used(path: &Path, used: u32) {
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(raw) = serde_json::to_string(&TrialState { trial_used: used }) else {
        return;
    };
    let mut temp_name = path.as_os_str().to_os_string();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    if fs::write(&temp, raw).is_ok() {
        let _ = fs::rename(&temp, path);
    }
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
}
