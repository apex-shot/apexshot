//! The piece of the Linux background compositor the shared renderer needs.
//!
//! `render::paint_attached_frame_border` takes the card's floating-point
//! rectangle. The full `BackgroundComposition` lives behind the Appearance
//! backend, which the Windows editor does not wire yet.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
