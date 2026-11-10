//! The studied cursor spring: the damping that makes the edited cursor glide.
//!
//! The reference we studied does not move the cursor sprite straight to each
//! recorded point. It runs a spring against a planned path, so the cursor
//! eases toward a point, trails it by the spring's own lag, and comes to rest
//! exactly on it. The presets below are the reference's cursor animation
//! presets, and the default is the soft spring the reference uses when no
//! preset is chosen. [`super::cursor_planner`] runs the planner that feeds it.

/// The reference integrates the spring in four-millisecond buckets.
pub const SPRING_BUCKET_MS: f64 = 4.0;
/// Velocity and distance below which the reference snaps the spring to rest.
pub const REST_EPSILON: f64 = 0.01;

/// A spring, in the reference's units: stiffness and damping are per second,
/// mass in arbitrary units, and positions are pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f64,
    pub damping: f64,
    pub mass: f64,
}

impl SpringConfig {
    /// The reference's default cursor spring, used when no preset is chosen.
    pub const NONE: Self = Self {
        stiffness: 125.0,
        damping: 12.0,
        mass: 1.5,
    };
    /// The reference's "Rapid" cursor animation preset.
    pub const RAPID: Self = Self {
        stiffness: 530.0,
        damping: 40.0,
        mass: 1.0,
    };
    /// The reference's "Medium" cursor animation preset.
    pub const MEDIUM: Self = Self {
        stiffness: 340.0,
        damping: 60.0,
        mass: 3.0,
    };
    /// The reference's "Smooth" cursor animation preset.
    pub const SMOOTH: Self = Self {
        stiffness: 470.0,
        damping: 70.0,
        mass: 3.0,
    };

    /// The preset the editor's smoothing slider selects. The slider's bands
    /// walk the reference's cursor presets from its default spring to its
    /// softest one, so an existing project keeps a sensible setting.
    pub fn for_smooth(smooth: f64) -> Self {
        let smooth = if smooth.is_finite() {
            smooth.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if smooth < 0.25 {
            Self::NONE
        } else if smooth < 0.5 {
            Self::RAPID
        } else if smooth < 0.75 {
            Self::MEDIUM
        } else {
            Self::SMOOTH
        }
    }

    /// The lag the spring itself introduces, in milliseconds. The reference's
    /// planner seeds its lead search from it.
    pub fn lag_ms(self) -> f64 {
        if self.stiffness <= 0.0 || !self.stiffness.is_finite() {
            return 0.0;
        }
        (self.damping / self.stiffness * 1000.0).clamp(0.0, 600.0)
    }
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smoothing_slider_walks_the_presets() {
        assert_eq!(SpringConfig::for_smooth(0.1), SpringConfig::NONE);
        assert_eq!(SpringConfig::for_smooth(0.4), SpringConfig::RAPID);
        assert_eq!(SpringConfig::for_smooth(0.6), SpringConfig::MEDIUM);
        assert_eq!(SpringConfig::for_smooth(0.9), SpringConfig::SMOOTH);
    }

    #[test]
    fn the_default_spring_is_the_reference_default() {
        assert_eq!(SpringConfig::default(), SpringConfig::NONE);
        assert!((SpringConfig::NONE.lag_ms() - 96.0).abs() < 1e-9);
    }
}
