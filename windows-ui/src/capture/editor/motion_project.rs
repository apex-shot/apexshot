//! The slice of the Motion sidecar the still-image sidecar shares: the Cast
//! light layer's on-disk shape, copied from the Linux Motion project file so
//! both platforms read and write the same JSON.

use serde::{Deserialize, Serialize};

use crate::recording::editor::model::{MotionSceneShadow, MotionSceneShadowPreset};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionSceneShadowPresetFile {
    #[default]
    None,
    /// Retired pattern ids load as a soft cast instead of failing to parse.
    #[serde(alias = "diagonal", alias = "window", alias = "vignette")]
    Spread,
    /// The directional ids were edge casts; a contact shadow is the closest
    /// surviving look.
    #[serde(alias = "top", alias = "bottom", alias = "side")]
    Hug,
    Adaptive,
}

fn default_one() -> f64 {
    1.0
}

/// The image's shadow on disk: style, strength, and where the light sits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionSceneShadowFile {
    #[serde(default)]
    pub preset: MotionSceneShadowPresetFile,
    #[serde(default = "default_one")]
    pub opacity: f64,
    #[serde(default = "default_scene_shadow_light")]
    pub light_x: f64,
    #[serde(default = "default_scene_shadow_light")]
    pub light_y: f64,
}

fn default_scene_shadow_light() -> f64 {
    0.65
}

impl From<MotionSceneShadowPreset> for MotionSceneShadowPresetFile {
    fn from(value: MotionSceneShadowPreset) -> Self {
        match value {
            MotionSceneShadowPreset::None => Self::None,
            MotionSceneShadowPreset::Spread => Self::Spread,
            MotionSceneShadowPreset::Hug => Self::Hug,
            MotionSceneShadowPreset::Adaptive => Self::Adaptive,
        }
    }
}

impl From<MotionSceneShadowPresetFile> for MotionSceneShadowPreset {
    fn from(value: MotionSceneShadowPresetFile) -> Self {
        match value {
            MotionSceneShadowPresetFile::None => Self::None,
            MotionSceneShadowPresetFile::Spread => Self::Spread,
            MotionSceneShadowPresetFile::Hug => Self::Hug,
            MotionSceneShadowPresetFile::Adaptive => Self::Adaptive,
        }
    }
}

impl From<&MotionSceneShadow> for MotionSceneShadowFile {
    fn from(value: &MotionSceneShadow) -> Self {
        Self {
            preset: value.preset.into(),
            opacity: value.opacity,
            light_x: value.light.0,
            light_y: value.light.1,
        }
    }
}

impl From<&MotionSceneShadowFile> for MotionSceneShadow {
    fn from(value: &MotionSceneShadowFile) -> Self {
        Self {
            preset: value.preset.into(),
            opacity: value.opacity,
            light: (value.light_x, value.light_y),
        }
    }
}
