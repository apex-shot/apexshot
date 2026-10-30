//! Cloud page facade for the Windows build.
//!
//! Sign-in is real: the first-run onboarding links this PC through the browser
//! and stores the same account cache the Linux build uses. The upload and
//! gallery transports are not wired on Windows yet, so those pages report the
//! reason instead of pretending a connection succeeded.

#[path = "../../../src/cloud/account.rs"]
pub mod account;

#[path = "../../../src/cloud/auth.rs"]
pub mod auth;

pub mod usage;

pub mod listing {
    use std::path::PathBuf;

    pub use super::account::{is_subscribed_tier, CloudAccount};

    pub fn cached_thumbnail(_url: &str) -> Result<PathBuf, String> {
        Err("Cloud thumbnails are not available in the Windows build yet".to_string())
    }
}

pub mod xbackbone {
    use crate::config::AppConfig;

    pub fn test_connection(_config: &AppConfig) -> Result<(), String> {
        Err("XBackBone uploads are not available in the Windows build yet".to_string())
    }
}
