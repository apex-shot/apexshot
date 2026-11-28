//! Screenshot organizer: names, indexes, and scrubs captures.
//!
//! Every capture gets a descriptive filename a moment after it is saved, its
//! on-screen text becomes searchable, and secrets (emails, keys, card numbers,
//! passwords) are kept out of both the name and the index.
//!
//! The crate is platform-neutral and GUI-free. Callers supply the OCR result
//! ([`OcrPage`]) and what was in front when the capture was taken
//! ([`CaptureContext`]); the Linux app gets those from its own OCR engine and
//! window tracking, the Windows shell from the system OCR and the foreground
//! window. Nothing here touches the network.

pub mod ai;
pub mod index;
pub mod naming;
pub mod organize;
pub mod quality;
pub mod secrets;
pub mod sensitive;
mod types;

pub use sensitive::Sensitivity;
pub use types::{CaptureContext, OcrPage, TextLine};
