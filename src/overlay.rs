//! GTK4 overlay for interactive area selection
//!
//! This module provides a full-screen transparent window that allows users
//! to select a screen area using mouse drag. Only used for X11 backend.

mod api;
mod background;
pub(crate) mod drawing;
mod geometry;
mod hit_testing;
pub(crate) mod icons;
pub(crate) mod layout;
pub(crate) mod monitor_picker;
pub(crate) mod recording;
mod state;
mod window;

#[cfg(test)]
mod tests;

pub use api::{
    select_area, select_area_from_capture, select_area_from_capture_with_gtk,
    select_area_from_capture_with_gtk_on_monitor, select_area_from_image,
    select_crosshair_from_capture_with_gtk, select_crosshair_from_capture_with_gtk_on_monitor,
    select_window_from_capture_with_gtk, AreaSelector, OverlaySelection, SelectionArea,
    SelectionError, SelectionResult,
};
pub use monitor_picker::{select_target_monitor_choice, MonitorChoice};
