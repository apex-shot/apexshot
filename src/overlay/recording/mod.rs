//! Recording panel sub-module for the GTK4 capture overlay.
//!
//! This module contains recording-specific UI code separated from the
//! screenshot capture area overlay. The recording panel replaces the
//! screenshot toolbar when the user clicks the "Recording" tool.
//!
//! Drawing functions remain in overlay/drawing.rs due to complex
//! dependencies on shared drawing helpers.

pub(crate) mod hit_testing;
pub(crate) mod layout;
pub(crate) mod state;
