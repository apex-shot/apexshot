use super::{Compositor, WindowInfo};
use std::env;

#[derive(Debug)]
pub struct Cosmic;

impl Default for Cosmic {
    fn default() -> Self {
        Self::new()
    }
}

impl Cosmic {
    pub fn new() -> Self {
        Self
    }

    pub fn is_supported() -> bool {
        // COSMIC sets XDG_CURRENT_DESKTOP=COSMIC and typically runs cosmic-comp.
        env::var_os("XDG_CURRENT_DESKTOP")
            .map(|s| s == "COSMIC")
            .unwrap_or(false)
    }
}

impl Compositor for Cosmic {
    fn name(&self) -> &str {
        "COSMIC"
    }

    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        // COSMIC IPC is not yet implemented — fall back to the portal path.
        Ok(Vec::new())
    }

    fn get_active_window(&self) -> anyhow::Result<Option<WindowInfo>> {
        Ok(None)
    }

    fn is_running(&self) -> bool {
        Self::is_supported()
    }
}
