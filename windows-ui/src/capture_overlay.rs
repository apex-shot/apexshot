//! Minimal capture-overlay facade for the settings pages.
//!
//! The Windows shell has no custom shutter sound yet, so the Sounds group
//! disables its controls exactly as a Linux portal-only build does.

pub fn supports_custom_shutter_sound() -> bool {
    true
}
