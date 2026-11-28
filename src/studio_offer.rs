//! The Studio price as the app states it, and when the Black Friday card is
//! due.
//!
//! The offer lives only in its own small card, shown once a day after the
//! first capture. Everywhere else states the normal price.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

/// The checkout code for the Black Friday price.
pub const BLACK_FRIDAY_CODE: &str = "BLACKFRIDAY50";

/// The Black Friday offer ends when December 1 begins on the user's clock.
fn offer_ends() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 12, 1).expect("valid date")
}

fn offer_active_on(today: NaiveDate) -> bool {
    today < offer_ends()
}

/// The short price line, e.g. on the Studio page and purchase cards.
pub fn price_label() -> String {
    crate::i18n::t("$39 once")
}

/// The price sentence that leads the AI names trial-ended card.
pub fn ai_names_price_note() -> String {
    crate::i18n::t("$39 once. Studio also includes Motion export, Upscale, and Studio styles.")
}

/// The Studio page with the offer's checkout code attached. The page already
/// carries the Device ID as a query parameter, so the code joins that query.
pub fn black_friday_studio_url() -> String {
    let url = crate::studio_entitlement::studio_page_url();
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}code={BLACK_FRIDAY_CODE}")
}

/// True when the Black Friday card may show on `today`: the offer still runs,
/// the device has no Studio, setup is done, and the card has not shown today.
pub fn black_friday_card_due_on(
    today: NaiveDate,
    last_shown: Option<NaiveDate>,
    studio_active: bool,
    onboarding_complete: bool,
) -> bool {
    offer_active_on(today) && !studio_active && onboarding_complete && last_shown != Some(today)
}

/// True when the Black Friday card is still due today: the offer runs, the
/// device has no Studio, setup is done, and the card has not shown today.
///
/// This only asks; the card process records the day with
/// [`mark_black_friday_card_shown`] once it has claimed the promo slot and
/// actually opens.
pub fn black_friday_card_due() -> bool {
    let today = Local::now().date_naive();
    let last_shown = state_path().as_deref().and_then(read_last_shown);
    black_friday_card_due_on(
        today,
        last_shown,
        crate::studio_entitlement::studio_unlocked(),
        onboarding_complete(),
    )
}

/// Record that the card opened today.
///
/// The card itself calls this only after the promo slot is its own, so a card
/// that never opens does not spend the day's card and another capture can
/// still show it later.
pub fn mark_black_friday_card_shown() {
    let today = Local::now().date_naive();
    if let Some(path) = state_path() {
        let _ = write_last_shown(&path, today);
    }
}

fn onboarding_complete() -> bool {
    crate::onboarding::is_onboarding_complete()
}

/// The state file sits beside the AI names trial state, under the app's own
/// data directory.
fn state_path() -> Option<PathBuf> {
    crate::ai_names::ai_dir().map(|dir| dir.join("black_friday.json"))
}

#[derive(Serialize, Deserialize, Default)]
struct CardState {
    #[serde(default)]
    last_shown: Option<NaiveDate>,
}

fn read_last_shown(path: &Path) -> Option<NaiveDate> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<CardState>(&raw).ok()?.last_shown
}

/// Write the day through a temporary file so a crash never leaves half a
/// state file behind.
fn write_last_shown(path: &Path, day: NaiveDate) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(raw) = serde_json::to_string(&CardState {
        last_shown: Some(day),
    }) else {
        return false;
    };
    let mut temp_name = path.as_os_str().to_os_string();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    if fs::write(&temp, raw).is_err() {
        return false;
    }
    fs::rename(&temp, path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    #[test]
    fn the_offer_runs_through_november_30() {
        assert!(offer_active_on(day(11, 28)));
        assert!(offer_active_on(day(11, 30)));
        assert!(!offer_active_on(day(12, 1)));
        assert!(!offer_active_on(day(12, 25)));
    }

    #[test]
    fn the_card_is_due_the_first_time_today() {
        assert!(black_friday_card_due_on(day(11, 28), None, false, true));
    }

    #[test]
    fn the_card_shows_only_once_a_day() {
        let today = day(11, 28);
        assert!(!black_friday_card_due_on(today, Some(today), false, true));
    }

    #[test]
    fn the_card_is_due_again_the_next_day() {
        assert!(black_friday_card_due_on(
            day(11, 29),
            Some(day(11, 28)),
            false,
            true
        ));
    }

    #[test]
    fn the_card_is_never_due_after_the_offer() {
        for date in [day(12, 1), day(12, 2), day(12, 25)] {
            assert!(!black_friday_card_due_on(date, None, false, true));
        }
    }

    #[test]
    fn the_card_is_never_due_for_studio_owners() {
        assert!(!black_friday_card_due_on(day(11, 28), None, true, true));
    }

    #[test]
    fn the_card_waits_for_onboarding_to_finish() {
        assert!(!black_friday_card_due_on(day(11, 28), None, false, false));
    }

    #[test]
    fn the_last_shown_day_survives_the_state_file() {
        let path = std::env::temp_dir()
            .join(format!(
                "apexshot-black-friday-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or(0)
            ))
            .join("black_friday.json");
        assert_eq!(read_last_shown(&path), None);
        assert!(write_last_shown(&path, day(11, 28)));
        assert_eq!(read_last_shown(&path), Some(day(11, 28)));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
