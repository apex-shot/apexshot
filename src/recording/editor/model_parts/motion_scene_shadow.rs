/// How the image's shadow falls on the background. The card is the occluder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionSceneShadowPreset {
    #[default]
    None,
    /// A wide, soft cast that reaches well past the card.
    Spread,
    /// A tight contact shadow that follows the card's edge.
    Hug,
    /// A middle falloff: soft like Spread, held as close as Hug.
    Adaptive,
}

/// Light position in `-1..=1` on both axes, y down. The shadow falls opposite.
pub const MIN_SCENE_SHADOW_LIGHT: f64 = -1.0;
pub const MAX_SCENE_SHADOW_LIGHT: f64 = 1.0;

impl MotionSceneShadowPreset {
    pub const ALL: [Self; 4] = [Self::None, Self::Spread, Self::Hug, Self::Adaptive];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Spread => "Spread",
            Self::Hug => "Hug",
            Self::Adaptive => "Adaptive",
        }
    }

    /// Blur, offset and alpha as fractions of the card's short edge.
    pub fn style(self) -> (f64, f64, f64) {
        match self {
            Self::None => (0.0, 0.0, 0.0),
            Self::Spread => (0.22, 0.10, 1.0),
            Self::Hug => (0.045, 0.02, 1.0),
            Self::Adaptive => (0.12, 0.06, 0.75),
        }
    }
}

/// One shadow cast by the image, with light position in -1..1, y down.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionSceneShadow {
    pub preset: MotionSceneShadowPreset,
    pub opacity: f64,
    /// `(0, 0)` places the light straight on and centers the shadow.
    pub light: (f64, f64),
}

impl Default for MotionSceneShadow {
    fn default() -> Self {
        Self {
            preset: MotionSceneShadowPreset::None,
            opacity: 1.0,
            light: (0.65, 0.65),
        }
    }
}

impl MotionSceneShadow {
    /// Changing the style preserves opacity and light position.
    pub fn select_preset(&mut self, preset: MotionSceneShadowPreset) {
        self.preset = preset;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Resolve to `(opacity, blur, offset)` for the card's short edge.
    pub fn resolved(&self, short: f64) -> (f64, f64, (f64, f64)) {
        let (blur, offset, alpha) = self.preset.style();
        let short = short.max(1.0);
        (
            self.opacity.clamp(0.0, 1.0) * alpha,
            blur * short,
            (
                -self.light.0 * offset * short,
                -self.light.1 * offset * short,
            ),
        )
    }

    pub fn clamped(&self) -> Self {
        Self {
            preset: self.preset,
            opacity: if self.opacity.is_finite() {
                self.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            },
            light: (
                finite_clamped(self.light.0, MIN_SCENE_SHADOW_LIGHT, MAX_SCENE_SHADOW_LIGHT),
                finite_clamped(self.light.1, MIN_SCENE_SHADOW_LIGHT, MAX_SCENE_SHADOW_LIGHT),
            ),
        }
    }
}

fn finite_clamped(value: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        0.0
    }
}
