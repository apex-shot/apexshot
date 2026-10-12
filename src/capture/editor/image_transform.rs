use serde::{Deserialize, Serialize};

use crate::recording::editor::model::{
    MotionTransform, DEFAULT_MOTION_END_PERSPECTIVE, MAX_MOTION_POS, MAX_MOTION_YAW,
    MIN_MOTION_POS, MIN_MOTION_YAW,
};

/// A still image's frozen Motion camera pose, stored with its annotation sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StaticImageTransform {
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub perspective: f64,
    pub pos_x: f64,
    pub pos_y: f64,
}

impl Default for StaticImageTransform {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            perspective: DEFAULT_MOTION_END_PERSPECTIVE,
            pos_x: 0.0,
            pos_y: 0.0,
        }
    }
}

impl StaticImageTransform {
    pub(crate) fn is_identity(self) -> bool {
        self.yaw.abs() < 1e-7
            && self.pitch.abs() < 1e-7
            && self.roll.abs() < 1e-7
            && self.pos_x.abs() < 1e-7
            && self.pos_y.abs() < 1e-7
    }

    pub(crate) fn sanitized(self) -> Self {
        let finite = |value: f64, fallback: f64| {
            if value.is_finite() {
                value
            } else {
                fallback
            }
        };
        Self {
            yaw: finite(self.yaw, 0.0).clamp(MIN_MOTION_YAW, MAX_MOTION_YAW),
            pitch: finite(self.pitch, 0.0).clamp(MIN_MOTION_YAW, MAX_MOTION_YAW),
            roll: finite(self.roll, 0.0).clamp(MIN_MOTION_YAW, MAX_MOTION_YAW),
            perspective: finite(self.perspective, DEFAULT_MOTION_END_PERSPECTIVE).clamp(0.0, 1.0),
            pos_x: finite(self.pos_x, 0.0).clamp(MIN_MOTION_POS, MAX_MOTION_POS),
            pos_y: finite(self.pos_y, 0.0).clamp(MIN_MOTION_POS, MAX_MOTION_POS),
        }
    }

    pub(crate) fn motion_transform(self) -> MotionTransform {
        let transform = self.sanitized();
        MotionTransform {
            scale: 1.0,
            rotation_x: transform.pitch,
            rotation_y: transform.yaw,
            rotation_z: transform.roll,
            perspective: transform.perspective,
            pos_x: transform.pos_x,
            pos_y: transform.pos_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StaticImageTransform;
    use crate::recording::editor::model::DEFAULT_MOTION_END_PERSPECTIVE;

    #[test]
    fn frozen_default_pose_is_identity_with_motion_perspective() {
        let transform = StaticImageTransform::default();
        assert!(transform.is_identity());
        assert_eq!(transform.perspective, DEFAULT_MOTION_END_PERSPECTIVE);
        let motion = transform.motion_transform();
        assert_eq!(motion.rotation_x, 0.0);
        assert_eq!(motion.rotation_y, 0.0);
        assert_eq!(motion.rotation_z, 0.0);
        assert_eq!(motion.perspective, DEFAULT_MOTION_END_PERSPECTIVE);
    }

    #[test]
    fn static_pose_uses_motion_angle_position_and_perspective_limits() {
        let transform = StaticImageTransform {
            yaw: 90.0,
            pitch: -90.0,
            roll: f64::INFINITY,
            perspective: 2.0,
            pos_x: -3.0,
            pos_y: f64::NAN,
        }
        .sanitized();
        assert_eq!(transform.yaw, 24.0);
        assert_eq!(transform.pitch, -24.0);
        assert_eq!(transform.roll, 0.0);
        assert_eq!(transform.perspective, 1.0);
        assert_eq!(transform.pos_x, -1.0);
        assert_eq!(transform.pos_y, 0.0);
    }

    #[test]
    fn missing_frozen_pose_fields_load_the_motion_defaults() {
        let transform: StaticImageTransform = serde_json::from_str("{}").unwrap();
        assert_eq!(transform, StaticImageTransform::default());
    }
}
