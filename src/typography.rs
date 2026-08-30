//! Central UI typography for every ApexShot-drawn surface.
//!
//! Cairo overlay text (capture overlay, recording panel, settings menu,
//! countdown, editor timeline) selects this family so the app renders with
//! one deliberate voice instead of whatever fontconfig maps the generic
//! "Sans" alias to (Noto Sans on most modern installs).
//!
//! Inter ships as a deb dependency (`fonts-inter`), is vendored into the
//! Flatpak, and fontconfig falls back to the system sans everywhere else.

/// The ApexShot UI font family used by all Cairo-drawn text.
pub const UI_FONT_FAMILY: &str = "Inter";
