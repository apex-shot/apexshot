//! Recording constants shared with the settings pages.
//!
//! The Windows recorder itself lives in the C++ shell; the settings window only
//! needs the option counts and the availability answer.

pub const VIDEO_MAX_RES_OPTION_COUNT: usize = 7;
pub const VIDEO_FPS_OPTION_COUNT: usize = 4;

pub const FEDORA_RECORDING_UNSUPPORTED_MSG: &str =
    "Video recording is not supported here. Screenshots still work.";

pub fn recording_supported() -> bool {
    true
}
