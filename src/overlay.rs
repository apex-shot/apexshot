//! GTK4 overlay for interactive area selection
//!
//! This module provides a full-screen transparent window that allows users
//! to select a screen area using mouse drag. Only used for X11 backend.

mod api;
mod background;
mod drawing;
mod geometry;
mod hit_testing;
mod icons;
mod layout;
pub(crate) mod recording;
mod state;
mod window;

#[cfg(test)]
mod tests;

pub use api::{
    select_area, select_area_from_capture, select_area_from_capture_with_gtk,
    select_area_from_image, select_crosshair_from_capture_with_gtk, AreaSelector, SelectionArea,
    SelectionError, SelectionResult,
};
