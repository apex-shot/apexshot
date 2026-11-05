//! The source-upscale option enum, shared with the Linux editor's types.
//!
//! The Windows image editor does not offer neural upscaling yet; the enum
//! exists so the shared annotation types serialize the same way on both
//! platforms. `factor` keeps the call sites identical for when it lands.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Optional offline neural enhancement applied at export time.
pub enum SourceUpscale {
    #[default]
    Off,
    Neural2x,
    Neural4x,
}

impl SourceUpscale {
    /// The exported image's enlargement factor.
    pub fn factor(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::Neural2x => 2,
            Self::Neural4x => 4,
        }
    }
}
