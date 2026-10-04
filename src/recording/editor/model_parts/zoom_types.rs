#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ZoomCameraSettings {
    pub speed: f64,
    pub smoothness: f64,
    pub start_early: bool,
    pub motion_blur: f64,
}

impl Default for ZoomCameraSettings {
    fn default() -> Self {
        Self {
            speed: 1.0,
            smoothness: 0.5,
            start_early: false,
            motion_blur: 0.0,
        }
    }
}

impl ZoomCameraSettings {
    pub fn clamped(self) -> Self {
        let defaults = Self::default();
        Self {
            speed: if self.speed.is_finite() {
                self.speed.clamp(0.25, 3.0)
            } else {
                defaults.speed
            },
            smoothness: if self.smoothness.is_finite() {
                self.smoothness.clamp(0.0, 1.0)
            } else {
                defaults.smoothness
            },
            start_early: self.start_early,
            motion_blur: if self.motion_blur.is_finite() {
                self.motion_blur.clamp(0.0, 1.0)
            } else {
                defaults.motion_blur
            },
        }
    }

    pub fn spring(self) -> CameraSpring {
        let settings = self.clamped();
        CameraSpring {
            stiffness: 125.0 * settings.speed * settings.speed,
            damping: (4.0 + 16.0 * settings.smoothness) * settings.speed,
            mass: 1.5,
        }
    }

    pub fn lead_in_seconds(self) -> f64 {
        if self.start_early {
            0.35
        } else {
            0.0
        }
    }
}

/// Where a zoom clip came from.
///
/// `mode` describes camera behavior — whether the clip follows the pointer.
/// Origin describes ownership, which is what automatic generation acts on: it
/// may replace only clips it created and nobody has touched since, so a
/// user-added or edited Auto zoom survives a re-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomOrigin {
    /// Placed by automatic suggestion and not edited since.
    Generated,
    /// Created or edited by the user.
    User,
    /// Loaded from a project written before origin existed. Treated as the
    /// user's work — an unknown clip is never safe to replace.
    #[default]
    Legacy,
}

/// The footage a generated clip was placed to frame, in source seconds.
///
/// A clip's span is a composition time, so trimming the head, re-cutting,
/// reordering, or retiming the video slides different footage under a clip that
/// only remembers where it sits on the timeline. An anchor is the source
/// interval the generator chose, which `reproject_anchored_zooms` turns back
/// into a span whenever the composition changes. It holds the plain source
/// interval rather than a segment identity: a clip follows its footage through
/// a re-cut, and is dropped only when the composition no longer plays that
/// footage at all.
///
/// `None` means the span is the user's own composition time — a clip they
/// placed or dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomAnchor {
    pub source_start: f64,
    pub source_end: f64,
}

/// The style a newly placed zoom opens with.
///
/// Captured from the last zoom the user edited, so a level or motion they
/// settle on carries to the next zoom they add instead of resetting to the
/// factory default every time. The studied editor's remembered style is the
/// same shape: zoom level plus the instant-vs-animated flag (alongside its
/// own presentation options, which have no counterpart here).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomStyle {
    pub scale: f64,
    pub easing: ZoomEasing,
    pub ease_ms: u32,
    /// Remembered snap choice. A generated zoom always opens animated; a
    /// zoom the user adds inherits whatever the last edit settled on.
    pub instant: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoomClip {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    pub ease_ms: u32,
    pub easing: ZoomEasing,
    pub mode: ZoomMode,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub perspective: f64,
    /// Whether the camera snaps instead of gliding.
    ///
    /// Ported from the studied editor's per-zoom instant flag: when set, the
    /// zoom opens and closes without an eased scale ramp, never morphs from
    /// a neighbour, and the follow camera jumps to the movement-group centre
    /// instead of chasing it on a spring. Older projects predate the flag
    /// and load it as `false`, keeping their current eased motion.
    pub instant: bool,
    /// Disabled from the clip's context menu. A hidden clip keeps its place on
    /// the timeline but stops feeding the preview and the export, so the work
    /// behind it survives a temporary mute.
    pub hidden: bool,
    /// Ownership, separate from camera behavior. See [`ZoomOrigin`].
    pub origin: ZoomOrigin,
    /// Footage this clip follows through composition edits. `None` for a clip
    /// the user placed or dragged. See [`ZoomAnchor`].
    pub anchor: Option<ZoomAnchor>,
}

impl Default for ZoomClip {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: DEFAULT_ZOOM_DURATION_SECONDS,
            scale: DEFAULT_ZOOM_SCALE,
            center: (0.0, 0.0),
            ease_ms: DEFAULT_ZOOM_EASE_MS,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Manual,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            perspective: 0.0,
            instant: false,
            hidden: false,
            origin: ZoomOrigin::default(),
            anchor: None,
        }
    }
}

impl ZoomClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn card_pose(&self) -> MotionTransform {
        MotionTransform {
            rotation_x: self.rotation_x,
            rotation_y: self.rotation_y,
            rotation_z: self.rotation_z,
            perspective: self.perspective,
            ..MotionTransform::default()
        }
    }

    pub fn has_card_motion(&self) -> bool {
        self.rotation_x.abs() + self.rotation_y.abs() + self.rotation_z.abs() + self.perspective
            > 0.04
    }
}

#[cfg(test)]
mod zoom_camera_settings_tests {
    use super::*;

    #[test]
    fn zoom_camera_defaults_match_the_shared_spring() {
        let settings = ZoomCameraSettings::default();
        assert_eq!(settings.spring(), CAMERA_FOLLOW_SPRING);
        assert_eq!(settings.lead_in_seconds(), 0.0);
    }

    #[test]
    fn zoom_camera_speed_and_smoothness_map_to_the_spring() {
        let settings = ZoomCameraSettings {
            speed: 2.0,
            smoothness: 0.75,
            start_early: true,
            motion_blur: 0.4,
        };
        assert_eq!(
            settings.spring(),
            CameraSpring {
                stiffness: 500.0,
                damping: 32.0,
                mass: 1.5,
            }
        );
        assert_eq!(settings.lead_in_seconds(), 0.35);
    }
}
