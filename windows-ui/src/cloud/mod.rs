//! Cloud transport for the Windows build.
//!
//! Sign-in, uploads and the XBackBone client are the same source modules the
//! Linux build compiles, attached here through `#[path]`. Only the gallery
//! transport stays a Windows stub until the History Cloud page has a backend.

#[path = "../../../src/cloud/account.rs"]
pub mod account;

#[path = "../../../src/cloud/apexshot.rs"]
#[rustfmt::skip]
pub mod apexshot;

#[path = "../../../src/cloud/auth.rs"]
#[rustfmt::skip]
pub mod auth;

#[path = "../../../src/cloud/destination.rs"]
#[rustfmt::skip]
pub mod destination;

pub mod helper;

#[path = "../../../src/cloud/upgrade.rs"]
#[rustfmt::skip]
pub mod upgrade;

#[path = "../../../src/cloud/upload.rs"]
#[rustfmt::skip]
pub mod upload;

pub mod usage;

#[path = "../../../src/cloud/xbackbone.rs"]
#[rustfmt::skip]
pub mod xbackbone;

pub mod listing {
    use std::path::PathBuf;

    pub use super::account::{is_subscribed_tier, CloudAccount};

    pub fn cached_thumbnail(_url: &str) -> Result<PathBuf, String> {
        Err("Cloud thumbnails are not available in the Windows build yet".to_string())
    }
}
