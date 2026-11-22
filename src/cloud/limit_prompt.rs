//! When the Cloud limit card may interrupt, and the near-limit heads-up.
//!
//! The card itself lives in [`super::upgrade`]. This module owns the small
//! state file that keeps a limit card to once per kind per day and a
//! near-limit card to once per kind per calendar month, plus the account fetch
//! that turns a successful upload into a usage line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::AppConfig;

use super::account::is_subscribed_tier;
use super::destination::Destination;
use super::upload::{CloudLimit, CloudLimitKind};

/// How long a full limit card stays quiet after it was shown.
pub const REPEAT_WINDOW_SECS: i64 = 24 * 60 * 60;

/// The usage lines and cards a successful upload earned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UploadHeadsUp {
    pub lines: Vec<String>,
    pub cards: Vec<CloudLimit>,
}

impl UploadHeadsUp {
    /// Append the usage lines to a notification body.
    pub fn append_to(&self, body: &str) -> String {
        if self.lines.is_empty() {
            return body.to_string();
        }
        let mut out = body.to_string();
        for line in &self.lines {
            out.push('\n');
            out.push_str(line);
        }
        out
    }
}

/// The persisted prompt history. Keys are `upgrade:<kind>` and
/// `near:<kind>:<YYYY-MM>`; values are Unix timestamps.
#[derive(Debug, Clone, Default)]
pub struct PromptLog {
    entries: BTreeMap<String, i64>,
}

impl PromptLog {
    pub fn from_entries(entries: BTreeMap<String, i64>) -> Self {
        Self { entries }
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str::<BTreeMap<String, i64>>(&raw)
            .map(Self::from_entries)
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_vec_pretty(&self.entries).map_err(std::io::Error::other)?;
        std::fs::write(path, raw)
    }

    pub fn should_show_upgrade(&self, kind: CloudLimitKind, now: i64) -> bool {
        match self.entries.get(&upgrade_key(kind)) {
            Some(last) => now.saturating_sub(*last) >= REPEAT_WINDOW_SECS,
            None => true,
        }
    }

    pub fn mark_upgrade(&mut self, kind: CloudLimitKind, now: i64) {
        self.entries.insert(upgrade_key(kind), now);
    }

    pub fn should_show_near(&self, kind: CloudLimitKind, month: &str) -> bool {
        !self.entries.contains_key(&near_key(kind, month))
    }

    pub fn mark_near(&mut self, kind: CloudLimitKind, month: &str, now: i64) {
        self.entries.insert(near_key(kind, month), now);
    }
}

fn upgrade_key(kind: CloudLimitKind) -> String {
    format!("upgrade:{}", kind.as_arg())
}

fn near_key(kind: CloudLimitKind, month: &str) -> String {
    format!("near:{}:{month}", kind.as_arg())
}

/// The cache file shared by both platforms.
pub fn state_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("apexshot")
        .join("cloud_limit_prompt.json")
}

/// The current month as `YYYY-MM`, in the user's local time.
pub fn current_month_key() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

/// Check and record a full limit card for `kind`.
pub fn allow_upgrade_prompt(kind: CloudLimitKind) -> bool {
    let path = state_path();
    let now = chrono::Utc::now().timestamp();
    let mut log = PromptLog::load_from(&path);
    if !log.should_show_upgrade(kind, now) {
        return false;
    }
    log.mark_upgrade(kind, now);
    if let Err(error) = log.save_to(&path) {
        eprintln!("[cloud] Could not record the limit prompt: {error}");
    }
    true
}

/// Check and record a near-limit card for `kind` in `month`.
pub fn allow_near_prompt(kind: CloudLimitKind, month: &str) -> bool {
    let path = state_path();
    let now = chrono::Utc::now().timestamp();
    let mut log = PromptLog::load_from(&path);
    if !log.should_show_near(kind, month) {
        return false;
    }
    log.mark_near(kind, month, now);
    if let Err(error) = log.save_to(&path) {
        eprintln!("[cloud] Could not record the near-limit prompt: {error}");
    }
    true
}

/// Fetch the account and describe how close it is to either free-tier limit.
///
/// Runs on a worker thread, never the UI thread. Returns `None` for a paid
/// tier, an XBackBone destination, a failed fetch, or an account comfortably
/// under both limits.
pub fn usage_heads_up(config: &AppConfig) -> Option<UploadHeadsUp> {
    if Destination::from_config(config) != Destination::ApexShot {
        return None;
    }
    let snapshot = super::usage::fetch_for_heads_up(config)?;
    if is_subscribed_tier(&snapshot.tier) {
        return None;
    }

    let mut heads_up = UploadHeadsUp::default();
    let month = current_month_key();

    if snapshot.limits.max_uploads > 0 {
        let used = snapshot.usage.uploads.max(0) as u64;
        let max = snapshot.limits.max_uploads as u64;
        let percentage = used.saturating_mul(100);
        if percentage >= max.saturating_mul(80) {
            heads_up.lines.push(crate::i18n::tfmt(
                "{used} of {max} free uploads used this month.",
                &[("used", &used.to_string()), ("max", &max.to_string())],
            ));
        }
        if percentage >= max.saturating_mul(90)
            && allow_near_prompt(CloudLimitKind::MonthlyUploads, &month)
        {
            heads_up.cards.push(CloudLimit {
                kind: CloudLimitKind::MonthlyUploads,
                message: String::new(),
                used: Some(used),
                max: Some(max),
                resets_at: snapshot.usage.month_resets_at.clone(),
            });
        }
    }

    if snapshot.limits.max_storage_bytes > 0 {
        let used = snapshot.usage.storage_bytes.max(0) as u64;
        let max = snapshot.limits.max_storage_bytes as u64;
        let percentage = used.saturating_mul(100);
        if percentage >= max.saturating_mul(80) {
            let used_text = super::upgrade::format_storage_size(used);
            let max_text = super::upgrade::format_storage_size(max);
            heads_up.lines.push(crate::i18n::tfmt(
                "{used} of {max} free Cloud storage used.",
                &[("used", &used_text), ("max", &max_text)],
            ));
        }
        if percentage >= max.saturating_mul(90)
            && allow_near_prompt(CloudLimitKind::Storage, &month)
        {
            heads_up.cards.push(CloudLimit {
                kind: CloudLimitKind::Storage,
                message: String::new(),
                used: Some(used),
                max: Some(max),
                resets_at: None,
            });
        }
    }

    if heads_up.lines.is_empty() && heads_up.cards.is_empty() {
        None
    } else {
        Some(heads_up)
    }
}

/// Start the standalone card for `limit`, reaped on its own thread.
pub fn spawn_card(limit: &CloudLimit, almost: bool) -> bool {
    let binary = card_binary();
    let mut command = std::process::Command::new(binary);
    #[cfg(not(target_os = "windows"))]
    command.arg("cloud-limit");
    #[cfg(target_os = "windows")]
    command.arg("--cloud-limit");
    command.arg(limit.kind.as_arg());
    if let Some(used) = limit.used {
        command.arg("--used").arg(used.to_string());
    }
    if let Some(max) = limit.max {
        command.arg("--max").arg(max.to_string());
    }
    if let Some(resets_at) = limit.resets_at.as_deref().filter(|value| !value.is_empty()) {
        command.arg("--resets-at").arg(resets_at);
    }
    if !limit.message.trim().is_empty() {
        command.arg("--message").arg(&limit.message);
    }
    if almost {
        command.arg("--almost");
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(error) => {
            eprintln!("[cloud] Could not open the limit card: {error}");
            false
        }
    }
}

fn card_binary() -> PathBuf {
    #[cfg(not(target_os = "windows"))]
    {
        crate::update_window::binary_path()
    }
    #[cfg(target_os = "windows")]
    {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot-settings.exe"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> BTreeMap<String, i64> {
        BTreeMap::new()
    }

    #[test]
    fn a_full_limit_card_waits_a_day_per_kind() {
        let mut log = PromptLog::from_entries(entries());
        let now = 1_000_000;
        assert!(log.should_show_upgrade(CloudLimitKind::MonthlyUploads, now));
        log.mark_upgrade(CloudLimitKind::MonthlyUploads, now);

        assert!(!log.should_show_upgrade(CloudLimitKind::MonthlyUploads, now + 60));
        assert!(
            !log.should_show_upgrade(CloudLimitKind::MonthlyUploads, now + REPEAT_WINDOW_SECS - 1)
        );
        assert!(log.should_show_upgrade(CloudLimitKind::MonthlyUploads, now + REPEAT_WINDOW_SECS));

        // The other kind keeps its own window.
        assert!(log.should_show_upgrade(CloudLimitKind::Storage, now));
    }

    #[test]
    fn a_near_limit_card_shows_once_per_calendar_month_per_kind() {
        let mut log = PromptLog::from_entries(entries());
        assert!(log.should_show_near(CloudLimitKind::MonthlyUploads, "2026-11"));
        log.mark_near(CloudLimitKind::MonthlyUploads, "2026-11", 1);

        assert!(!log.should_show_near(CloudLimitKind::MonthlyUploads, "2026-11"));
        assert!(log.should_show_near(CloudLimitKind::MonthlyUploads, "2026-12"));
        assert!(log.should_show_near(CloudLimitKind::Storage, "2026-11"));
    }

    #[test]
    fn prompt_history_round_trips_through_the_cache_file() {
        let path = std::env::temp_dir().join(format!(
            "apexshot-limit-prompt-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let mut log = PromptLog::from_entries(entries());
        log.mark_upgrade(CloudLimitKind::Storage, 42);
        log.mark_near(CloudLimitKind::MonthlyUploads, "2026-11", 43);
        log.save_to(&path).expect("save prompt history");

        let loaded = PromptLog::load_from(&path);
        assert!(!loaded.should_show_upgrade(CloudLimitKind::Storage, 42));
        assert!(!loaded.should_show_near(CloudLimitKind::MonthlyUploads, "2026-11"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn usage_lines_are_appended_after_the_share_link() {
        let heads_up = UploadHeadsUp {
            lines: vec!["20 of 20 free uploads used this month.".to_string()],
            cards: Vec::new(),
        };
        assert_eq!(
            heads_up.append_to("https://apexshot.org/s/x"),
            "https://apexshot.org/s/x\n20 of 20 free uploads used this month."
        );
        assert_eq!(UploadHeadsUp::default().append_to("body"), "body");
    }
}
