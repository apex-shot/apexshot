//! The Studio price as the app states it, including a time-limited offer.
//!
//! Every place that shows the price asks here, so an offer starts and ends in
//! one spot. The checkout on the website charges the price; this only says it.

use chrono::{Local, NaiveDate};

/// The checkout code for the Black Friday price. New and existing users both
/// see it wherever the price is shown.
pub const BLACK_FRIDAY_CODE: &str = "BLACKFRIDAY50";

/// The Black Friday offer ends when December 1 begins on the user's clock.
fn offer_ends() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 12, 1).expect("valid date")
}

fn offer_active_on(today: NaiveDate) -> bool {
    today < offer_ends()
}

/// True while the Black Friday price applies.
pub fn black_friday_active() -> bool {
    offer_active_on(Local::now().date_naive())
}

/// The short price line, e.g. on the Studio page and purchase cards.
pub fn price_label() -> String {
    if black_friday_active() {
        crate::i18n::tfmt(
            "$19.50 once with code {code} · 50% off for Black Friday",
            &[("code", BLACK_FRIDAY_CODE)],
        )
    } else {
        crate::i18n::t("$39 once")
    }
}

/// The price sentence that leads the AI names trial-ended card.
pub fn ai_names_price_note() -> String {
    if black_friday_active() {
        crate::i18n::tfmt(
            "$19.50 once with code {code}, 50% off for Black Friday. Studio also includes Motion export, Upscale, and Studio styles.",
            &[("code", BLACK_FRIDAY_CODE)],
        )
    } else {
        crate::i18n::t("$39 once. Studio also includes Motion export, Upscale, and Studio styles.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_runs_through_november_30() {
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        assert!(offer_active_on(day(11, 28)));
        assert!(offer_active_on(day(11, 30)));
        assert!(!offer_active_on(day(12, 1)));
        assert!(!offer_active_on(day(12, 25)));
    }
}
