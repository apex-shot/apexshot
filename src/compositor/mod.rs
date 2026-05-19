use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub mod hyprland;
pub mod niri;
pub mod river;
pub mod sway;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub class: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub workspace: String,
    pub is_active: bool,
}

pub trait Compositor: Debug + Send + Sync {
    /// Get the name of the compositor
    fn name(&self) -> &str;

    /// Get all windows from all workspaces
    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>>;

    /// Get the currently focused window
    fn get_active_window(&self) -> anyhow::Result<Option<WindowInfo>>;

    /// Check if this compositor is currently running
    fn is_running(&self) -> bool;
}

/// Detect the current compositor and return an implementation
pub fn detect_compositor() -> Option<Box<dyn Compositor>> {
    if hyprland::Hyprland::is_supported() {
        return Some(Box::new(hyprland::Hyprland::new()));
    }
    if sway::Sway::is_supported() {
        return Some(Box::new(sway::Sway::new()));
    }
    if niri::Niri::is_supported() {
        return Some(Box::new(niri::Niri::new()));
    }
    if river::River::is_supported() {
        return Some(Box::new(river::River::new()));
    }
    None
}
