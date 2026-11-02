//! Studio entitlement verification shared with the other desktop builds.

#[path = "../../src/studio_entitlement.rs"]
mod shared;

pub use shared::*;

#[cfg(test)]
pub(crate) use shared::test_studio_entitlement;
