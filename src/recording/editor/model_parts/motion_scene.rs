#[derive(Debug, Clone, PartialEq, Default)]
pub enum MotionBackgroundFillType {
    /// Motion renders this as black, rather than transparency.
    #[default]
    None,
    Color,
    Gradient,
    Wallpaper,
    Image,
}

/// The scene fields carried by the Motion appearance model.  This is
/// deliberately separate from the animated card transform: the background is
/// a compositor layer, shared by preview and export.
use crate::capture::editor::types::{BackgroundAlignment, FrameStyle};

#[derive(Debug, Clone, PartialEq)]
pub struct MotionAppearance {
    pub source_upscale: crate::capture::editor::upscale::SourceUpscale,
    pub background_padding: f64,
    /// Legacy card inset (0-200 slider units) and placement inside the
    /// canvas. Motion has no controls for them, but it must render them or a
    /// restored static layout would silently change on entering Motion.
    pub background_insert: f64,
    pub background_alignment: BackgroundAlignment,
    pub background_fill_type: MotionBackgroundFillType,
    pub background_color: [f64; 4],
    /// The background gradient, shared verbatim with the video editor's
    /// Custom Wallpaper editor (stops, kind, angle, reverse) so both popovers
    /// edit the same spec.
    pub gradient: VideoGradient,
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
    pub frame_style: FrameStyle,
    /// The compositor's card-shadow inputs. Derived from the shadow layer by
    /// `MotionSceneShadow::resolved` (see `EditorState::sync_shadow_fields` and
    /// the Appearance panel); no control writes them directly, so the layer is
    /// the only shadow description in the app.
    pub shadow_blur: f64,
    pub shadow_opacity: f64,
    pub shadow_position: (f64, f64),
}

impl Default for MotionAppearance {
    fn default() -> Self {
        Self {
            source_upscale: Default::default(),
            // Preserve the established Motion card framing until the user
            // changes it; unlike the fill, there is no recoverable numeric
            // default for this field. Capture's Background tool overrides this
            // to 0px (see `MotionSession::new`).
            background_padding: 96.0,
            background_insert: 0.0,
            background_alignment: BackgroundAlignment::Center,
            background_fill_type: MotionBackgroundFillType::None,
            background_color: [0.0, 0.0, 0.0, 1.0],
            gradient: VideoGradient {
                stops: vec![
                    GradientStop::new(0.0, 0, 0, 0),
                    GradientStop::new(1.0, 0, 0, 0),
                ],
                ..VideoGradient::default()
            },
            selected_gradient_preset_index: None,
            wallpaper_image_name: None,
            custom_background_image: None,
            background_blur: 0.0,
            background_noise: 0.0,
            border_radius: 0.0,
            frame_style: FrameStyle::Default,
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
/// is intentionally kept out of `MotionAppearance`: the model carries a
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

/// The image-Motion `framePresetId` section with recovered labels
/// Standard, Instagram, X (Twitter), and YouTube. The binary does not expose
/// the presets' dimensions or safe areas, so the output aspects below are
/// Apexshot design decisions: each preset re-fits the Motion scene into a
/// centered social output format, while Standard keeps the original canvas.
///
/// The generic ratio presets (16:9 through 9:16 plus Custom) back the Frame
/// picker grid. Legacy Instagram/X/YouTube variants are kept for files that
/// already persist them; the picker maps them to the same aspect as their
/// canonical equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionFramePreset {
    #[default]
    Standard,
    Instagram,
    X,
    YouTube,
    SixteenNine,
    ThreeTwo,
    FourThree,
    FiveFour,
    OneOne,
    FourFive,
    ThreeFour,
    TwoThree,
    NineSixteen,
    TenTwentyOne,
    YouTubeBanner,
    YouTubeThumbnail,
    YouTubeVideo,
    TwitterTweet,
    TwitterCover,
    InstagramPost,
    InstagramPortrait,
    InstagramStory,
    PinterestLong,
    PinterestOptimal,
    PinterestSquare,
    Custom,
}

impl MotionFramePreset {
    /// Target output width/height ratio; `None` keeps the source canvas.
    /// Custom has no fixed ratio: read it from `MotionFrame::effective_aspect`.
    pub fn aspect(self) -> Option<f64> {
        match self {
            Self::Standard => None,
            Self::Custom => None,
            Self::Instagram | Self::OneOne | Self::InstagramPost | Self::PinterestSquare => {
                Some(1.0)
            }
            // X link-card ratio.
            Self::X => Some(1200.0 / 628.0),
            Self::YouTube
            | Self::SixteenNine
            | Self::YouTubeBanner
            | Self::YouTubeThumbnail
            | Self::YouTubeVideo
            | Self::TwitterTweet => Some(16.0 / 9.0),
            Self::TwitterCover => Some(3.0),
            Self::ThreeTwo => Some(3.0 / 2.0),
            Self::FourThree => Some(4.0 / 3.0),
            Self::FiveFour => Some(5.0 / 4.0),
            Self::FourFive | Self::InstagramPortrait => Some(4.0 / 5.0),
            Self::ThreeFour => Some(3.0 / 4.0),
            Self::TwoThree | Self::PinterestOptimal => Some(2.0 / 3.0),
            Self::NineSixteen | Self::InstagramStory => Some(9.0 / 16.0),
            Self::TenTwentyOne | Self::PinterestLong => Some(10.0 / 21.0),
        }
    }

    /// Short ratio label for the picker tiles; Standard/Custom are handled
    /// by the caller because they show names, not ratios.
    pub fn ratio_label(self) -> &'static str {
        match self {
            Self::Standard => "Original",
            Self::Custom => "Custom",
            Self::Instagram | Self::OneOne | Self::InstagramPost | Self::PinterestSquare => "1:1",
            Self::X => "1.91:1",
            Self::YouTube
            | Self::SixteenNine
            | Self::YouTubeBanner
            | Self::YouTubeThumbnail
            | Self::YouTubeVideo
            | Self::TwitterTweet => "16:9",
            Self::TwitterCover => "3:1",
            Self::ThreeTwo => "3:2",
            Self::FourThree => "4:3",
            Self::FiveFour => "5:4",
            Self::FourFive | Self::InstagramPortrait => "4:5",
            Self::ThreeFour => "3:4",
            Self::TwoThree | Self::PinterestOptimal => "2:3",
            Self::NineSixteen | Self::InstagramStory => "9:16",
            Self::TenTwentyOne | Self::PinterestLong => "10:21",
        }
    }

    /// Fixed pixel dimensions for social sizes. Generic ratios size from the
    /// export budget instead; see MotionFrame::output_size.
    pub fn fixed_dimensions(self) -> Option<(i32, i32)> {
        match self {
            Self::YouTubeBanner => Some((2560, 1440)),
            Self::YouTubeThumbnail => Some((1280, 720)),
            Self::YouTubeVideo => Some((1920, 1080)),
            // Spec is 1200x675 (odd height); yuv420p needs even sizes so the
            // frame evens it to 1200x676 on output.
            Self::TwitterTweet => Some((1200, 676)),
            Self::TwitterCover => Some((1500, 500)),
            Self::InstagramPost => Some((1080, 1080)),
            Self::InstagramPortrait => Some((1080, 1350)),
            Self::InstagramStory => Some((1080, 1920)),
            Self::PinterestLong => Some((1000, 2100)),
            Self::PinterestOptimal => Some((1000, 1500)),
            Self::PinterestSquare => Some((1000, 1000)),
            _ => None,
        }
    }
}

/// A separately persisted Frame layer, matching the
/// `frameSnapshot` contract rather than a field of the Appearance scene.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionFrame {
    pub preset: MotionFramePreset,
    pub custom_width: u32,
    pub custom_height: u32,
}

impl Default for MotionFrame {
    fn default() -> Self {
        Self {
            preset: MotionFramePreset::Standard,
            custom_width: 1920,
            custom_height: 1440,
        }
    }
}

impl MotionFrame {
    /// Effective scene aspect for preview/export. Standard keeps the source
    /// canvas; Custom uses the manual W/H inputs; every other preset uses
    /// its fixed ratio.
    pub fn effective_aspect(&self) -> Option<f64> {
        match self.preset {
            MotionFramePreset::Standard => None,
            MotionFramePreset::Custom => {
                if self.custom_width > 0 && self.custom_height > 0 {
                    Some(f64::from(self.custom_width) / f64::from(self.custom_height))
                } else {
                    None
                }
            }
            preset => preset.aspect(),
        }
    }

    fn even(value: f64) -> i32 {
        ((value.round() as i32) / 2 * 2).max(2)
    }

    /// Output canvas for this frame. Landscape keeps the established 1920px
    /// long edge; portrait is sized by width so vertical frames export at the
    /// 1080x1920 social standard (9:16) instead of a narrow letterbox, and a
    /// ratio taller than 9:16 caps its long edge at the same 1920 budget.
    /// Dimensions are rounded to even values because the MP4 encoder's
    /// yuv420p pixel format requires even sizes.
    ///
    /// `source_aspect` is the composed Motion canvas aspect of the still.
    /// Standard has no ratio of its own, so it uses that instead of the
    /// fixed 16:9 default: the export keeps the canvas the preview shows
    /// rather than re-framing the composition into a different shape.
    pub fn output_size_for(&self, source_aspect: f64) -> (i32, i32) {
        if let Some((w, h)) = self.preset.fixed_dimensions() {
            return (Self::even(f64::from(w)), Self::even(f64::from(h)));
        }
        match self.preset {
            MotionFramePreset::Custom => {
                let w = (self.custom_width.clamp(16, 7680) as f64).round();
                let h = (self.custom_height.clamp(16, 7680) as f64).round();
                // Scale down if the long edge exceeds the export budget,
                // preserving the manual ratio.
                let longest = w.max(h);
                let (w, h) = if longest > 1920.0 {
                    let scale = 1920.0 / longest;
                    (w * scale, h * scale)
                } else {
                    (w, h)
                };
                (Self::even(w), Self::even(h))
            }
            _ => match self.effective_aspect().or(Some(source_aspect)) {
                None => (1920, 1080),
                Some(aspect) if aspect >= 1.0 => (1920, Self::even(1920.0 / aspect)),
                Some(aspect) => {
                    let height = 1080.0 / aspect;
                    if height > 1920.0 {
                        (Self::even(1920.0 * aspect), 1920)
                    } else {
                        (1080, Self::even(height))
                    }
                }
            },
        }
    }

    /// Output canvas without a source still's canvas aspect. Standard keeps
    /// the established 16:9 export budget here; the editor's Frame panel shows
    /// the still's own W/H for Standard instead (see `build_motion_appearance`).
    pub fn output_size(&self) -> (i32, i32) {
        self.output_size_for(16.0 / 9.0)
    }
}

/// How the image's shadow falls on the background. The image is the window:
/// the shadow is cast by the card's own rectangle, never by a pattern drawn
/// across the scene.
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

/// Light position on the adjustment pad, `-1..=1` on both axes, y down. The
/// shadow falls opposite the light, so a dot at the bottom left casts toward
/// the top right.
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

    /// Geometry for this style, as fractions of the card's short edge:
    /// `(blur, offset, alpha)`.
    ///
    /// Spread is wide and soft; Hug keeps a short falloff so the shadow reads
    /// as contact; Adaptive sits between them and gives up some strength so it
    /// stays readable on both light and dark backgrounds.
    pub fn style(self) -> (f64, f64, f64) {
        match self {
            Self::None => (0.0, 0.0, 0.0),
            Self::Spread => (0.22, 0.10, 1.0),
            Self::Hug => (0.045, 0.02, 1.0),
            Self::Adaptive => (0.12, 0.06, 0.75),
        }
    }
}

/// The image's shadow: the style it is drawn with, how strong it is, and where
/// the light sits. One layer shared by both editors; the card is always the
/// occluder, so the shadow falls behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionSceneShadow {
    pub preset: MotionSceneShadowPreset,
    pub opacity: f64,
    /// Light position on the pad in `-1..1`, y down. `(0, 0)` is a light
    /// straight on, which leaves the shadow centred under the card.
    pub light: (f64, f64),
}

impl Default for MotionSceneShadow {
    fn default() -> Self {
        Self {
            preset: MotionSceneShadowPreset::None,
            opacity: 1.0,
            // Light from the lower right: the cast falls toward the upper
            // left, which is the shipped look of the adjustment pad.
            light: (0.65, 0.65),
        }
    }
}

impl MotionSceneShadow {
    /// Adopt a style. Opacity and the light position belong to the user, so a
    /// style change leaves them alone.
    pub fn select_preset(&mut self, preset: MotionSceneShadowPreset) {
        self.preset = preset;
    }

    /// Restore every control to its shipped default: no shadow, full strength,
    /// light at the lower right.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Resolve this shadow into the compositor's card-shadow fields for a card
    /// `short` pixels across: `(opacity, blur, offset)`. Blur and offset are
    /// canvas units, so a style keeps its proportions at any image size.
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

    /// Clamp every stored value into the range the painter expects.
    pub fn clamped(&self) -> Self {
        Self {
            preset: self.preset,
            opacity: if self.opacity.is_finite() {
                self.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            },
            light: (
                finite_clamped(
                    self.light.0,
                    MIN_SCENE_SHADOW_LIGHT,
                    MAX_SCENE_SHADOW_LIGHT,
                ),
                finite_clamped(
                    self.light.1,
                    MIN_SCENE_SHADOW_LIGHT,
                    MAX_SCENE_SHADOW_LIGHT,
                ),
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

#[derive(Debug, Clone, PartialEq)]
pub struct MotionState {
    pub duration: f64,
    pub segments: Vec<MotionSegment>,
    pub playhead: f64,
    /// The compositor-level `perspectiveIntensity`. It is deliberately
    /// independent of timed effect segments so every camera move shares one
    /// projection depth.
    pub perspective_intensity: f64,
    pub motion_blur: f64,
    pub motion_blur_settings: MotionBlurSettings,
    pub selected: Option<usize>,
    pub text_segments: Vec<MotionTextSegment>,
    pub selected_text: Option<usize>,
    pub appearance: MotionAppearance,
    pub watermark: MotionWatermark,
    pub frame: MotionFrame,
    pub scene_shadow: MotionSceneShadow,
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
            selected: None,
            text_segments: Vec::new(),
            selected_text: None,
            appearance: MotionAppearance::default(),
            watermark: MotionWatermark::default(),
            frame: MotionFrame::default(),
            scene_shadow: MotionSceneShadow::default(),
        }
    }
}
