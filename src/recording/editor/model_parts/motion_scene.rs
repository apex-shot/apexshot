#[derive(Debug, Clone, PartialEq, Default)]
pub enum MotionBackgroundFillType {
    /// Shotbase renders this as black in Motion, rather than transparency.
    #[default]
    None,
    Color,
    Gradient,
    Wallpaper,
    Image,
}

/// The scene fields carried by Shotbase's Motion appearance model.  This is
/// deliberately separate from the animated card transform: the background is
/// a compositor layer, shared by preview and export.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionAppearance {
    pub background_padding: f64,
    pub background_fill_type: MotionBackgroundFillType,
    pub background_color: [f64; 4],
    pub gradient_color_1: [f64; 4],
    pub gradient_color_2: [f64; 4],
    pub selected_gradient_preset_index: Option<usize>,
    pub wallpaper_image_name: Option<String>,
    pub custom_background_image: Option<String>,
    pub background_blur: f64,
    pub background_noise: f64,
    /// Corner radius applied to the captured image card itself; the
    /// background fill stays a full rectangle.
    pub border_radius: f64,
    pub border_thickness: f64,
    pub border_fill_color: [f64; 4],
    pub shadow_blur: f64,
    pub shadow_opacity: f64,
    pub shadow_position: (f64, f64),
}

impl Default for MotionAppearance {
    fn default() -> Self {
        Self {
            // Preserve the established Motion card framing until the user
            // changes it; unlike the fill, Shotbase's binary does not expose
            // a recoverable numeric default for this field.
            background_padding: 96.0,
            background_fill_type: MotionBackgroundFillType::None,
            background_color: [0.0, 0.0, 0.0, 1.0],
            gradient_color_1: [0.0, 0.0, 0.0, 1.0],
            gradient_color_2: [0.0, 0.0, 0.0, 1.0],
            selected_gradient_preset_index: None,
            wallpaper_image_name: None,
            custom_background_image: None,
            background_blur: 0.0,
            background_noise: 0.0,
            border_radius: 0.0,
            border_thickness: 0.0,
            // Opaque by default so raising the thickness immediately shows a
            // border instead of silently stroking in invisible white.
            border_fill_color: [1.0, 1.0, 1.0, 1.0],
            shadow_blur: 16.0,
            shadow_opacity: 0.0,
            shadow_position: (0.0, 16.0),
        }
    }
}

/// A separately composited image layer for capture-image Motion.  The source
/// is intentionally kept out of `MotionAppearance`: Shotbase carries a
/// dedicated watermark snapshot rather than treating it as card decoration.
///
/// ApexShot stores normalized card-space values so one watermark setup has the
/// same composition in the interactive preview and the fixed-size MP4 export.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionWatermark {
    /// User-owned image resource selected for this Motion scene.
    pub image_file_name: Option<String>,
    /// Fraction of the source card width occupied by the watermark.
    pub size: f64,
    /// Minimum distance from each card edge, as a fraction of card width.
    pub inset: f64,
    /// Normalized card-space center, from the top-left `(0, 0)` to the
    /// bottom-right `(1, 1)`.
    pub position: (f64, f64),
}

impl Default for MotionWatermark {
    fn default() -> Self {
        Self {
            image_file_name: None,
            size: 0.18,
            inset: 0.04,
            position: (0.90, 0.90),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionState {
    pub duration: f64,
    pub segments: Vec<MotionSegment>,
    pub playhead: f64,
    /// Shotbase's compositor-level `perspectiveIntensity`. It is deliberately
    /// independent of timed effect segments so every camera move shares one
    /// projection depth.
    pub perspective_intensity: f64,
    pub motion_blur: f64,
    pub motion_blur_settings: MotionBlurSettings,
    pub transform_timing: MotionEffectTransformTiming,
    pub selected: Option<usize>,
    pub text_segments: Vec<MotionTextSegment>,
    pub selected_text: Option<usize>,
    pub appearance: MotionAppearance,
    pub watermark: MotionWatermark,
}

impl Default for MotionState {
    fn default() -> Self {
        Self {
            duration: DEFAULT_MOTION_DURATION_SECONDS,
            segments: Vec::new(),
            playhead: 0.0,
            perspective_intensity: DEFAULT_MOTION_END_PERSPECTIVE,
            motion_blur: 0.0,
            motion_blur_settings: MotionBlurSettings::default(),
            transform_timing: MotionEffectTransformTiming::default(),
            selected: None,
            text_segments: Vec::new(),
            selected_text: None,
            appearance: MotionAppearance::default(),
            watermark: MotionWatermark::default(),
        }
    }
}
