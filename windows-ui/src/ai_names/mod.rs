//! Optional AI names: a small open model and the llama.cpp server, downloaded
//! once on request and run only on this computer.

pub mod card;
pub mod catalog;
pub mod hardware;
pub mod install;
pub mod server;
pub mod trial;

pub use card::run_card;
pub use catalog::*;
pub use hardware::*;
pub use install::*;
pub use server::*;
pub use trial::*;
pub use trial::{claim_card, CardKind};
