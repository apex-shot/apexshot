//! The signed-in account as the server describes it, and the tier predicate
//! that turns it into an entitlement answer.
//!
//! Shared by the Linux and Windows builds: both read the same account fields
//! and cache them in the same config, so the type lives outside either
//! platform's read-side facade.

use serde::Deserialize;

use crate::config::AppConfig;

/// The only tier that is not a paid plan.
const FREE_TIER: &str = "free";

/// The signed-in account as the server describes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudAccount {
    #[serde(default)]
    pub email: String,
    /// `free`, `pro`, `team`, … Empty when the server did not say.
    #[serde(default)]
    pub tier: String,
}

impl CloudAccount {
    /// True when this account is on a paid plan.
    pub fn is_subscribed(&self) -> bool {
        is_subscribed_tier(&self.tier)
    }

    /// Cache the account on `config` so UI can render without a network call.
    /// Keeps the legacy `cloud_pro_plan` flag in sync with the tier, and leaves
    /// existing values alone for fields the server did not send.
    pub fn apply_to_config(&self, config: &mut AppConfig) {
        let email = self.email.trim();
        if !email.is_empty() {
            config.cloud_user_email = email.to_string();
        }
        let tier = self.tier.trim().to_ascii_lowercase();
        if !tier.is_empty() {
            config.cloud_pro_plan = is_subscribed_tier(&tier);
            config.cloud_plan_tier = tier;
        }
    }
}

/// Is this tier a paid plan? Anything other than `free` counts — so a tier the
/// desktop app has not heard of yet still unlocks paid features. An absent tier
/// is not a claim of payment, so it does not.
pub fn is_subscribed_tier(tier: &str) -> bool {
    let tier = tier.trim().to_ascii_lowercase();
    !tier.is_empty() && tier != FREE_TIER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paid_tiers_are_recognised() {
        assert!(!is_subscribed_tier("free"));
        assert!(is_subscribed_tier("pro"));
        assert!(is_subscribed_tier("team"));
        assert!(is_subscribed_tier("enterprise"));
    }

    #[test]
    fn tier_matching_ignores_case_and_padding() {
        assert!(!is_subscribed_tier("  FREE "));
        assert!(is_subscribed_tier(" Pro "));
        assert!(is_subscribed_tier("TEAM"));
        assert!(!is_subscribed_tier(""));
        assert!(!is_subscribed_tier("   "));
    }

    #[test]
    fn account_caches_email_and_tier() {
        let mut config = AppConfig::default();
        CloudAccount {
            email: "user@example.com".to_string(),
            tier: "Team".to_string(),
        }
        .apply_to_config(&mut config);

        assert_eq!(config.cloud_user_email, "user@example.com");
        assert_eq!(config.cloud_plan_tier, "team");
        assert!(config.cloud_pro_plan);
    }

    #[test]
    fn empty_fields_leave_the_cache_alone() {
        let mut config = AppConfig {
            cloud_user_email: "kept@example.com".to_string(),
            cloud_plan_tier: "pro".to_string(),
            ..AppConfig::default()
        };

        CloudAccount {
            email: "  ".to_string(),
            tier: String::new(),
        }
        .apply_to_config(&mut config);

        assert_eq!(config.cloud_user_email, "kept@example.com");
        assert_eq!(config.cloud_plan_tier, "pro");
    }
}
