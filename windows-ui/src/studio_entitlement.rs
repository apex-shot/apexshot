//! Studio page facade.
//!
//! Studio license verification is a Linux-side subsystem. The Windows page
//! shows the supported "this build can't verify licenses" state instead of
//! offering an activation flow that cannot work.

use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum StudioStatus {
    ConfigurationUnavailable,
    NotActivated,
    InvalidEntitlement,
    DeviceMismatch,
    Active { updates_until: i64 },
    UpdateAccessEnded { updates_until: i64 },
}

impl StudioStatus {
    pub fn message(self) -> String {
        match self {
            Self::ConfigurationUnavailable => {
                "Studio isn't available in this build configuration. Free capture and editing remain available.".into()
            }
            Self::NotActivated => {
                "Studio isn't activated. Free capture, editing, recording, and clean exports continue to work offline.".into()
            }
            Self::InvalidEntitlement => {
                "The saved Studio entitlement couldn't be verified. Import a valid entitlement to activate this build.".into()
            }
            Self::DeviceMismatch => {
                "This licence is bound to another device. Open the Studio settings on this machine, copy its Device ID, and claim the licence for it at apexshot.org/studio.".into()
            }
            Self::Active { updates_until } => format!(
                "Studio is active for this build. Included updates run through {updates_until}."
            ),
            Self::UpdateAccessEnded { updates_until } => format!(
                "Studio is active, but included updates ended on {updates_until}. Install a covered build to use Studio output."
            ),
        }
    }
}

#[derive(Debug)]
pub enum StudioEntitlementError {
    ConfigurationUnavailable,
    NotActivated,
    InvalidEntitlement,
    DeviceMismatch,
}

impl fmt::Display for StudioEntitlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigurationUnavailable => {
                formatter.write_str("Studio activation is unavailable in this build configuration")
            }
            Self::NotActivated => formatter.write_str("Studio isn't activated on this device"),
            Self::InvalidEntitlement => {
                formatter.write_str("The Studio entitlement is invalid or could not be verified")
            }
            Self::DeviceMismatch => {
                formatter.write_str("This Studio licence is bound to a different device")
            }
        }
    }
}

impl std::error::Error for StudioEntitlementError {}

pub fn current_status() -> StudioStatus {
    StudioStatus::ConfigurationUnavailable
}

pub fn activate_token(_token: &str) -> Result<(), StudioEntitlementError> {
    Err(StudioEntitlementError::ConfigurationUnavailable)
}

pub fn stored_token() -> Option<String> {
    None
}

pub fn device_id() -> Option<String> {
    None
}
