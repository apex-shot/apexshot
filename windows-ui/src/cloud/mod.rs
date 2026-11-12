//! Cloud transport for the Windows build.
//!
//! Sign-in, uploads and the XBackBone client are the same source modules the
//! Linux build compiles, attached here through `#[path]`, as is the uploads
//! listing the History window's Cloud page reads.

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

#[path = "../../../src/cloud/listing.rs"]
#[rustfmt::skip]
pub mod listing;

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
