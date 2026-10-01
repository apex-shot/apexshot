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
    /// Disabled from the clip's context menu. A hidden clip keeps its place on
    /// the timeline but stops feeding the preview and the export, so the work
    /// behind it survives a temporary mute.
    pub hidden: bool,
    /// Ownership, separate from camera behavior. See [`ZoomOrigin`].
    pub origin: ZoomOrigin,
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
            hidden: false,
            origin: ZoomOrigin::default(),
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
