#![allow(deprecated)]

//! Rust UI shell for the Windows build.
//!
//! This crate owns the GTK4 surfaces that the Windows package runs as a
//! separate process. The settings pages are shared verbatim with the Linux
//! binary (`#[path]` includes), while the platform modules below provide the
//! handful of Linux APIs those pages expect.

pub mod app_identity;
pub mod capture;
pub mod capture_overlay;
pub mod cloud;
#[path = "../../src/config.rs"]
pub mod config;
pub mod daemon;
pub mod history;
pub mod i18n;
pub mod platform;
pub mod recording;
pub mod settings;
pub mod studio_entitlement;
#[path = "../../src/test_support.rs"]
pub mod test_support;
pub mod utils;
