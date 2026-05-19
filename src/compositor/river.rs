use super::{Compositor, WindowInfo};
use std::env;

#[derive(Debug)]
pub struct River;

impl Default for River {
    fn default() -> Self {
        Self::new()
    }
}

impl River {
    pub fn new() -> Self {
        Self
    }

    pub fn is_supported() -> bool {
        env::var_os("RIVER_WLR_UNSTABLE_V1_PATH").is_some()
            || env::var_os("WAYLAND_DISPLAY").is_some()
                && env::var_os("XDG_CURRENT_DESKTOP")
                    .map(|s| s == "river")
                    .unwrap_or(false)
    }
}

impl Compositor for River {
    fn name(&self) -> &str {
        "River"
    }

    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        // River window listing requires wlr-foreign-toplevel-management.
        // For now, return empty list (snapping won't work, but hotkeys will).
        Ok(Vec::new())
    }

    fn get_active_window(&self) -> anyhow::Result<Option<WindowInfo>> {
        Ok(None)
    }

    fn is_running(&self) -> bool {
        Self::is_supported()
    }
}
