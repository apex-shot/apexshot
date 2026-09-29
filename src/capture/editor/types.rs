use super::numbering_style::{NumberSize, NumberingStyle};
use crate::recording::editor::model::VideoGradient;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error("Screenshot file not found: {0}")]
    MissingFile(PathBuf),

    #[error("Failed to load image: {0}")]
    ImageLoad(String),

    #[error("Failed to save image: {0}")]
    ImageSave(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundStyle {
    None,
    /// A gradient fill, carried as the shared `VideoGradient` spec (stops,
    /// kind, angle) rather than a preset index, so the static preview and its
    /// export describe the same fill the Motion side does.
    Gradient(VideoGradient),
    Wallpaper(PathBuf),
    Blurred(usize),
    PlainColor(DrawColor),
}

/// Minimum surround kept between the screenshot and the background edges
/// whenever a fill is active, in padding slider units (reference px against
/// a 400px long edge, exactly like padding itself). A canvas shaped by Frame
/// would otherwise let the image touch the background on the short axis at
/// padding 0, so layouts floor tiny paddings to this breathing room. Stored
/// padding values are untouched — sliders still read/write the real value;
/// only rendering uses the floored one.
pub const BACKGROUND_MIN_GAP: f64 = 8.0;

/// Rendering padding for a canvas: the user's value, floored to
/// [`BACKGROUND_MIN_GAP`] while a background fill is active so the image
/// never sits edge-to-edge against the fill on any Frame.
pub fn effective_background_padding(padding: f64, has_fill: bool) -> f64 {
    if has_fill {
        padding.max(BACKGROUND_MIN_GAP)
    } else {
        padding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundAlignment {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObfuscateMethod {
    Pixelate,
    Blur,
    Blackout,
}

#[allow(dead_code)]
impl ObfuscateMethod {
    pub fn display_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "Pixelate",
            ObfuscateMethod::Blur => "Blur",
            ObfuscateMethod::Blackout => "Blackout",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "obfuscate-pixelate",
            ObfuscateMethod::Blur => "obfuscate-blur",
            ObfuscateMethod::Blackout => "obfuscate-blackout",
        }
    }

    pub fn has_slider(&self) -> bool {
        !matches!(self, ObfuscateMethod::Blackout)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowStyle {
    Standard,
    Fancy,
    Curved,
    Double,
}

impl ArrowStyle {
    pub const ALL: [Self; 4] = [Self::Standard, Self::Fancy, Self::Curved, Self::Double];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Fancy => "Fancy",
            Self::Curved => "Curved",
            Self::Double => "Double",
        }
    }
}

/// Frame style preset for the screenshot card border.
///
/// Replaces the old freeform border color/thickness sliders: each preset maps
/// to an outside border (drawn *around* the image, not inside it) plus
/// optional outer accent strokes for stacked looks. The corner radius stays
/// user-adjustable via the Border Radius slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FrameStyle {
    #[default]
    Default,
    GlassLight,
    GlassDark,
    Liquid,
    InsetLight,
    InsetDark,
    Outline,
    Border,
    Retro,
    Card,
    Stack,
    Stack2,
}

/// One outer accent stroke: thickness in slider px, color, and gap in slider
/// px between the previous stroke's outer edge and this stroke's inner edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameOuterStroke {
    pub thickness: f64,
    pub color: DrawColor,
    pub gap: f64,
}

/// One backing sheet behind the card (Stack looks): same size as the card,
/// offset in fixed canvas px (not scaled with image size so the peek stays
/// modest on fullscreen shots), slight rotation in degrees, flat fill.
/// Sheets with `center_pivot` fan diagonally (Stack: top-right and
/// bottom-left peeks); the rest pivot at the hidden bottom-right corner and
/// fan top-left (Stack2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameBacking {
    pub offset_x: f64,
    pub offset_y: f64,
    pub rotation_deg: f64,
    pub center_pivot: bool,
    pub color: DrawColor,
}

/// Resolved border rendering for a [`FrameStyle`]: main outside border, up to
/// two outer accent strokes, and up to two backing sheets drawn
/// *behind* the card (Stack looks). Farthest backing sheet first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSpec {
    pub border_thickness: f64,
    pub border_color: DrawColor,
    /// True for Inset styles: the main border paints *inside* the image edge
    /// instead of outside it, so it needs no extra canvas.
    pub inset_border: bool,
    /// True for the glass family (Liquid, Glass Light, Glass Dark): the edge
    /// paints as glass instead of a flat stroke, so renderers hand the preset
    /// to `render::LiquidFrame` (Cairo previews) or `render::glass_layer`
    /// (static exports). The whitish/darkish read comes from `border_color`.
    pub liquid: bool,
    /// True for the frosted-glass siblings (Glass Light, Glass Dark): a
    /// heavily-blurred milky/smoked band like Shots.so frames, as opposed
    /// to Liquid's clear refractive edge. Only meaningful with `liquid`.
    pub frost: bool,
    pub outer1: Option<FrameOuterStroke>,
    pub outer2: Option<FrameOuterStroke>,
    pub backing1: Option<FrameBacking>,
    pub backing2: Option<FrameBacking>,
}

impl FrameStyle {
    pub const ALL: [Self; 12] = [
        Self::Default,
        Self::GlassLight,
        Self::GlassDark,
        Self::Liquid,
        Self::InsetLight,
        Self::InsetDark,
        Self::Outline,
        Self::Border,
        Self::Retro,
        Self::Card,
        Self::Stack,
        Self::Stack2,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::GlassLight => "Glass Light",
            Self::GlassDark => "Glass Dark",
            Self::Liquid => "Liquid",
            Self::InsetLight => "Inset Light",
            Self::InsetDark => "Inset Dark",
            Self::Outline => "Outline",
            Self::Border => "Border",
            Self::Retro => "Retro",
            Self::Card => "Card",
            Self::Stack => "Stack",
            Self::Stack2 => "Stack 2",
        }
    }

    pub fn spec(self) -> FrameSpec {
        match self {
            Self::Default => FrameSpec {
                border_thickness: 0.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.0),
                outer1: None,
                outer2: None,
                backing1: None,
                backing2: None,
            },
            // Glass Light: frosted glass like Shots.so frames in a 3px edge —
            // a heavily-blurred band carrying a whitish veil, edged by a
            // crisp specular rim. The backdrop smears through the frost
            // instead of bending like Liquid's refractive edge.
            Self::GlassLight => FrameSpec {
                border_thickness: 2.0,
                inset_border: false,
                liquid: true,
                frost: true,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.55),
                outer1: Some(FrameOuterStroke {
                    thickness: 1.0,
                    color: DrawColor::new(1.0, 1.0, 1.0, 0.95),
                    gap: 0.0,
                }),
                outer2: None,
                backing1: None,
                backing2: None,
            },
            // Glass Dark: the same 3px frost, smoked — the band deepens the
            // backdrop while the white speculars and rim keep the glass read.
            Self::GlassDark => FrameSpec {
                border_thickness: 2.0,
                inset_border: false,
                liquid: true,
                frost: true,
                border_color: DrawColor::new(0.05, 0.05, 0.07, 0.60),
                outer1: Some(FrameOuterStroke {
                    thickness: 1.0,
                    color: DrawColor::new(1.0, 1.0, 1.0, 0.9),
                    gap: 0.0,
                }),
                outer2: None,
                backing1: None,
                backing2: None,
            },
            // Liquid Glass (Apple/NSGlassEffectView language): a 3px glass
            // edge — a 2px clear refracted body capped by a 1px specular
            // rim. The body stays translucent so the backdrop reads through
            // it; the light lives in the top-weighted specular and the crisp
            // rim, visible all around the perimeter. Renderers expand this
            // into gradients (Cairo previews) or the refraction shader
            // (`render::glass_layer` in static exports) rather than a flat
            // stroke.
            Self::Liquid => FrameSpec {
                border_thickness: 2.0,
                inset_border: false,
                liquid: true,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.20),
                outer1: Some(FrameOuterStroke {
                    thickness: 1.0,
                    color: DrawColor::new(1.0, 1.0, 1.0, 0.9),
                    gap: 0.0,
                }),
                outer2: None,
                backing1: None,
                backing2: None,
            },
            Self::InsetLight => FrameSpec {
                border_thickness: 3.0,
                inset_border: true,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.85),
                outer1: None,
                outer2: None,
                backing1: None,
                backing2: None,
            },
            Self::InsetDark => FrameSpec {
                border_thickness: 3.0,
                inset_border: true,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(0.0, 0.0, 0.0, 0.85),
                outer1: None,
                outer2: None,
                backing1: None,
                backing2: None,
            },
            Self::Outline => FrameSpec {
                border_thickness: 0.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.0),
                outer1: Some(FrameOuterStroke {
                    thickness: 1.0,
                    color: DrawColor::new(0.7, 0.7, 0.7, 1.0),
                    gap: 2.0,
                }),
                outer2: None,
                backing1: None,
                backing2: None,
            },
            Self::Border => FrameSpec {
                border_thickness: 3.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(0.0, 0.0, 0.0, 1.0),
                outer1: None,
                outer2: None,
                backing1: None,
                backing2: None,
            },
            Self::Retro => FrameSpec {
                border_thickness: 3.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(0.0, 0.0, 0.0, 1.0),
                outer1: None,
                outer2: None,
                backing1: Some(FrameBacking {
                    offset_x: 28.0,
                    offset_y: 28.0,
                    rotation_deg: 0.0,
                    center_pivot: false,
                    color: DrawColor::new(0.0, 0.0, 0.0, 1.0),
                }),
                backing2: None,
            },
            Self::Card => FrameSpec {
                border_thickness: 0.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.0),
                outer1: None,
                outer2: None,
                backing1: Some(FrameBacking {
                    offset_x: -6.0,
                    offset_y: -16.0,
                    rotation_deg: -1.5,
                    center_pivot: false,
                    color: DrawColor::new(0.78, 0.78, 0.80, 1.0),
                }),
                backing2: None,
            },
            Self::Stack => FrameSpec {
                border_thickness: 0.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.0),
                outer1: None,
                outer2: None,
                backing1: Some(FrameBacking {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    rotation_deg: -2.0,
                    center_pivot: true,
                    color: DrawColor::new(0.75, 0.75, 0.77, 1.0),
                }),
                backing2: None,
            },
            Self::Stack2 => FrameSpec {
                border_thickness: 0.0,
                inset_border: false,
                liquid: false,
                frost: false,
                border_color: DrawColor::new(1.0, 1.0, 1.0, 0.0),
                outer1: None,
                outer2: None,
                backing1: Some(FrameBacking {
                    offset_x: -10.0,
                    offset_y: -28.0,
                    rotation_deg: -3.0,
                    center_pivot: false,
                    color: DrawColor::new(0.56, 0.56, 0.58, 1.0),
                }),
                backing2: Some(FrameBacking {
                    offset_x: -5.0,
                    offset_y: -14.0,
                    rotation_deg: -1.5,
                    center_pivot: false,
                    color: DrawColor::new(0.78, 0.78, 0.80, 1.0),
                }),
            },
        }
    }
}

/// Whether a frame style grows the canvas beyond the bare screenshot even
/// with no background: an outside border, accent strokes, or backing sheets.
/// `Default` with zero manual thickness needs nothing, so legacy no-background
/// exports stay pixel-identical.
pub fn frame_needs_canvas(style: FrameStyle, border_thickness: f64) -> bool {
    let spec = style.spec();
    if border_thickness > 0.01 && !spec.inset_border {
        return true;
    }
    spec.backing1.is_some()
        || spec.backing2.is_some()
        || spec.outer1.is_some()
        || spec.outer2.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Select,
    Background,
    Pen,
    Highlighter,
    Circle,
    Arrow,
    Line,
    Box,
    Text,
    Number,
    Obfuscate,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontStyle {
    Normal,
    Bold,
    Italic,
    BoldItalic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextDecoration {
    None,
    Underline,
    Strikethrough,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontSettings {
    pub family: String,
    pub size: f64,
    pub style: FontStyle,
    pub decoration: TextDecoration,
    pub alignment: TextAlignment,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            family: String::from("Sans"),
            size: 16.0,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeControlMode {
    Stroke,
    Obfuscate,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CropAspectRatio {
    Freeform,
    Original,
    Square,
    FourThree,
    SixteenNine,
    TwentyOneNine,
    ThreeTwo,
    NineSixteen,
    FiveFour,
    FourFive,
    ThreeFour,
    TwoThree,
    TenTwentyOne,
    ThreeOne,
}

impl CropAspectRatio {
    pub const ALL: [Self; 14] = [
        Self::Freeform,
        Self::Original,
        Self::Square,
        Self::FourThree,
        Self::SixteenNine,
        Self::TwentyOneNine,
        Self::ThreeTwo,
        Self::NineSixteen,
        Self::FiveFour,
        Self::FourFive,
        Self::ThreeFour,
        Self::TwoThree,
        Self::TenTwentyOne,
        Self::ThreeOne,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Freeform => "Freeform",
            Self::Original => "Original",
            Self::Square => "Square",
            Self::FourThree => "4:3",
            Self::SixteenNine => "16:9",
            Self::TwentyOneNine => "21:9",
            Self::ThreeTwo => "3:2",
            Self::NineSixteen => "9:16",
            Self::FiveFour => "5:4",
            Self::FourFive => "4:5",
            Self::ThreeFour => "3:4",
            Self::TwoThree => "2:3",
            Self::TenTwentyOne => "10:21",
            Self::ThreeOne => "3:1",
        }
    }

    pub fn aspect_ratio(self, image_width: i32, image_height: i32) -> Option<f64> {
        match self {
            Self::Freeform => None,
            Self::Original => {
                if image_width > 0 && image_height > 0 {
                    Some(image_width as f64 / image_height as f64)
                } else {
                    None
                }
            }
            Self::Square => Some(1.0),
            Self::FourThree => Some(4.0 / 3.0),
            Self::SixteenNine => Some(16.0 / 9.0),
            Self::TwentyOneNine => Some(21.0 / 9.0),
            Self::ThreeTwo => Some(3.0 / 2.0),
            Self::NineSixteen => Some(9.0 / 16.0),
            Self::FiveFour => Some(5.0 / 4.0),
            Self::FourFive => Some(4.0 / 5.0),
            Self::ThreeFour => Some(3.0 / 4.0),
            Self::TwoThree => Some(2.0 / 3.0),
            Self::TenTwentyOne => Some(10.0 / 21.0),
            Self::ThreeOne => Some(3.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectHandle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl DrawColor {
    pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    pub fn with_alpha(self, alpha: f64) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a: alpha,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PickerColorState {
    pub hue: f64,
    pub saturation: f64,
    pub value: f64,
    pub alpha: f64,
}

impl PickerColorState {
    pub fn from_color(color: DrawColor) -> Self {
        let (hue, saturation, value) = rgb_to_hsv(color.r, color.g, color.b);
        Self {
            hue,
            saturation,
            value,
            alpha: color.a.clamp(0.0, 1.0),
        }
    }

    pub fn to_color(self) -> DrawColor {
        let (r, g, b) = hsv_to_rgb(self.hue, self.saturation, self.value);
        DrawColor::new(r, g, b, self.alpha.clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ViewTransform {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub image_width: f64,
    pub image_height: f64,
    /// True when a background (wallpaper/padding) canvas is active. The
    /// annotation space stays in screenshot pixels, but negative / overflow
    /// coordinates address the background padding around the screenshot.
    pub has_background: bool,
    /// Canvas origin in view coordinates (top-left of the full background).
    pub canvas_offset_x: f64,
    /// Canvas origin in view coordinates.
    pub canvas_offset_y: f64,
    /// View scale for the full canvas (maps canvas px -> view px).
    pub canvas_scale: f64,
    /// Full virtual canvas size in canvas pixels.
    pub canvas_width: f64,
    /// Full virtual canvas size in canvas pixels.
    pub canvas_height: f64,
    /// Screenshot origin inside the canvas, in canvas pixels.
    pub image_rect_x: f64,
    /// Screenshot origin inside the canvas, in canvas pixels.
    pub image_rect_y: f64,
    /// Screenshot scale inside the canvas (insert). Maps screenshot px -> canvas px.
    pub canvas_draw_scale: f64,
}

impl ViewTransform {
    pub fn for_image(image_width: f64, image_height: f64) -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            image_width,
            image_height,
            has_background: false,
            canvas_offset_x: 0.0,
            canvas_offset_y: 0.0,
            canvas_scale: 1.0,
            canvas_width: image_width,
            canvas_height: image_height,
            image_rect_x: 0.0,
            image_rect_y: 0.0,
            canvas_draw_scale: 1.0,
        }
    }

    #[allow(dead_code)]
    pub fn fit(image_width: f64, image_height: f64, view_width: f64, view_height: f64) -> Self {
        if image_width <= 0.0 || image_height <= 0.0 || view_width <= 1.0 || view_height <= 1.0 {
            return Self::for_image(image_width.max(1.0), image_height.max(1.0));
        }

        let scale = (view_width / image_width)
            .min(view_height / image_height)
            .min(1.0);

        let draw_width = image_width * scale;
        let draw_height = image_height * scale;
        let offset_x = (view_width - draw_width) / 2.0;
        let offset_y = (view_height - draw_height) / 2.0;

        Self {
            scale,
            offset_x,
            offset_y,
            image_width,
            image_height,
            has_background: false,
            canvas_offset_x: offset_x,
            canvas_offset_y: offset_y,
            canvas_scale: scale,
            canvas_width: image_width,
            canvas_height: image_height,
            image_rect_x: 0.0,
            image_rect_y: 0.0,
            canvas_draw_scale: 1.0,
        }
    }

    /// Bounds of the full editable canvas expressed in screenshot pixels.
    /// Without a background this is exactly the screenshot; with a background
    /// the padding maps to negative / overflow coordinates so tools can place
    /// annotations on the wallpaper instead of clipping them away.
    pub fn canvas_bounds_in_image_coords(&self) -> (f64, f64, f64, f64) {
        if !self.has_background {
            return (0.0, 0.0, self.image_width, self.image_height);
        }
        let draw_scale = self.canvas_draw_scale.max(0.0001);
        let min_x = -self.image_rect_x / draw_scale;
        let min_y = -self.image_rect_y / draw_scale;
        let max_x = (self.canvas_width - self.image_rect_x) / draw_scale;
        let max_y = (self.canvas_height - self.image_rect_y) / draw_scale;
        (min_x, min_y, max_x, max_y)
    }

    pub fn contains_view(&self, point: Point) -> bool {
        if self.has_background {
            let draw_width = self.canvas_width * self.canvas_scale;
            let draw_height = self.canvas_height * self.canvas_scale;
            return point.x >= self.canvas_offset_x
                && point.y >= self.canvas_offset_y
                && point.x <= self.canvas_offset_x + draw_width
                && point.y <= self.canvas_offset_y + draw_height;
        }
        let draw_width = self.image_width * self.scale;
        let draw_height = self.image_height * self.scale;
        point.x >= self.offset_x
            && point.y >= self.offset_y
            && point.x <= self.offset_x + draw_width
            && point.y <= self.offset_y + draw_height
    }

    pub fn view_to_image(&self, point: Point) -> Point {
        let scale = self.scale.max(0.0001);
        Point {
            x: (point.x - self.offset_x) / scale,
            y: (point.y - self.offset_y) / scale,
        }
    }

    pub fn view_to_image_clamped(&self, point: Point) -> Point {
        let mut image_point = self.view_to_image(point);
        if self.has_background {
            let (min_x, min_y, max_x, max_y) = self.canvas_bounds_in_image_coords();
            let (lo_x, hi_x) = if min_x <= max_x {
                (min_x, max_x)
            } else {
                (max_x, min_x)
            };
            let (lo_y, hi_y) = if min_y <= max_y {
                (min_y, max_y)
            } else {
                (max_y, min_y)
            };
            image_point.x = image_point.x.clamp(lo_x, hi_x);
            image_point.y = image_point.y.clamp(lo_y, hi_y);
            return image_point;
        }
        image_point.x = image_point.x.clamp(0.0, self.image_width);
        image_point.y = image_point.y.clamp(0.0, self.image_height);
        image_point
    }

    #[allow(dead_code)]
    pub fn image_to_view(&self, point: Point) -> Point {
        Point {
            x: point.x * self.scale + self.offset_x,
            y: point.y * self.scale + self.offset_y,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn from_points(start: Point, end: Point) -> Option<Self> {
        let min_x = start.x.min(end.x).floor() as i32;
        let min_y = start.y.min(end.y).floor() as i32;
        let max_x = start.x.max(end.x).ceil() as i32;
        let max_y = start.y.max(end.y).ceil() as i32;
        let width = max_x - min_x;
        let height = max_y - min_y;

        if width <= 1 || height <= 1 {
            return None;
        }

        Some(Self {
            x: min_x,
            y: min_y,
            width,
            height,
        })
    }

    pub fn clamp_to(self, width: u32, height: u32) -> Option<Self> {
        let x0 = self.x.max(0).min(width as i32);
        let y0 = self.y.max(0).min(height as i32);
        let x1 = (self.x + self.width).max(0).min(width as i32);
        let y1 = (self.y + self.height).max(0).min(height as i32);

        let clamped_w = x1 - x0;
        let clamped_h = y1 - y0;
        if clamped_w <= 0 || clamped_h <= 0 {
            return None;
        }

        Some(Self {
            x: x0,
            y: y0,
            width: clamped_w,
            height: clamped_h,
        })
    }

    pub fn from_bounds(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Option<Self> {
        let x = min_x.floor() as i32;
        let y = min_y.floor() as i32;
        let width = (max_x.ceil() as i32) - x;
        let height = (max_y.ceil() as i32) - y;

        if width <= 1 || height <= 1 {
            return None;
        }

        Some(Self {
            x,
            y,
            width,
            height,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AnnotationAction {
    Pen {
        points: Vec<Point>,
        color: DrawColor,
        stroke_size: f64,
    },
    Highlighter {
        points: Vec<Point>,
        color: DrawColor,
        stroke_size: f64,
    },
    Circle {
        rect: Rect,
        color: DrawColor,
        stroke_size: f64,
        shadow: bool,
    },
    Line {
        start: Point,
        end: Point,
        color: DrawColor,
        stroke_size: f64,
        shadow: bool,
    },
    Arrow {
        start: Point,
        end: Point,
        color: DrawColor,
        stroke_size: f64,
        style: ArrowStyle,
        control_points: Option<Vec<Point>>,
        shadow: bool,
    },
    Box {
        rect: Rect,
        color: DrawColor,
        stroke_size: f64,
        shadow: bool,
    },
    #[allow(dead_code)]
    Text {
        position: Point,
        text: String,
        color: DrawColor,
        font: FontSettings,
        max_width: Option<f64>,
        shadow: bool,
        background_color: Option<DrawColor>,
    },
    Number {
        position: Point,
        number: u32,
        color: DrawColor,
        style: NumberingStyle,
        size: NumberSize,
        shadow: bool,
    },
    Obfuscate {
        rect: Rect,
        method: ObfuscateMethod,
        amount: f64,
    },
    Focus {
        rect: Rect,
        intensity: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MoveHandle {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeHandle {
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct TextEditBounds {
    pub rect: Rect,
    pub move_handles: Vec<(MoveHandle, Point)>,
    pub resize_handle: Option<(ResizeHandle, Point)>,
}

impl TextEditBounds {
    pub fn new(position: Point, width: f64, height: f64) -> Self {
        let mut bounds = Self {
            rect: Rect {
                x: position.x.round() as i32,
                y: position.y.round() as i32,
                width: width.round().max(1.0) as i32,
                height: height.round().max(1.0) as i32,
            },
            move_handles: vec![(MoveHandle::Left, position), (MoveHandle::Right, position)],
            resize_handle: Some((ResizeHandle::BottomRight, position)),
        };
        bounds.sync_handles();
        bounds
    }

    pub fn sync_handles(&mut self) {
        let x = self.rect.x as f64;
        let y = self.rect.y as f64;
        let w = self.rect.width.max(1) as f64;
        let h = self.rect.height.max(1) as f64;

        if let Some((_, point)) = self.move_handles.get_mut(0) {
            *point = Point { x, y: y + h / 2.0 };
        }
        if let Some((_, point)) = self.move_handles.get_mut(1) {
            *point = Point {
                x: x + w,
                y: y + h / 2.0,
            };
        }
        if let Some((_, point)) = &mut self.resize_handle {
            *point = Point { x: x + w, y: y + h };
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PersistedCustomColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PersistedCustomColorSlots {
    pub slots: Vec<Option<PersistedCustomColor>>,
}

pub fn normalize_hue(hue: f64) -> f64 {
    let mut normalized = hue % 360.0;
    if normalized < 0.0 {
        normalized += 360.0;
    }
    normalized
}

pub fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> (f64, f64, f64) {
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);

    if saturation <= f64::EPSILON {
        return (value, value, value);
    }

    let hue = normalize_hue(hue);
    let sector = hue / 60.0;
    let i = sector.floor() as i32;
    let f = sector - i as f64;

    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));

    match i {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    }
}

pub fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let r = r.clamp(0.0, 1.0);
    let g = g.clamp(0.0, 1.0);
    let b = b.clamp(0.0, 1.0);

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let hue = if delta <= f64::EPSILON {
        0.0
    } else if (max - r).abs() <= f64::EPSILON {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() <= f64::EPSILON {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let saturation = if max <= f64::EPSILON {
        0.0
    } else {
        delta / max
    };
    (normalize_hue(hue), saturation, max)
}

pub fn tool_uses_stroke_size(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Pen | Tool::Highlighter | Tool::Circle | Tool::Line | Tool::Arrow | Tool::Box
    )
}

/// Index into the editor toolbar `tool_buttons` vector built in `window/mod.rs`.
/// Keep this match arm order identical to that vector or active-tool highlighting breaks.
pub fn tool_button_index(tool: Tool) -> usize {
    match tool {
        Tool::Background => 0,
        Tool::Select => 1,
        Tool::Pen => 2,
        Tool::Box => 3,
        Tool::Circle => 4,
        Tool::Arrow => 5,
        Tool::Line => 6,
        Tool::Text => 7,
        Tool::Obfuscate => 8,
        Tool::Number => 9,
        Tool::Highlighter => 10,
        Tool::Focus => 11,
    }
}

pub fn tool_shortcut_target(key: char) -> Option<(Tool, usize)> {
    let tool = match key.to_ascii_lowercase() {
        '0' | '`' | 's' => Tool::Select,
        '1' | 'd' | 'p' => Tool::Pen,
        '2' | 't' => Tool::Text,
        '3' | 'l' => Tool::Line,
        '4' | 'a' => Tool::Arrow,
        '5' | 'r' => Tool::Box,
        '6' | 'o' => Tool::Circle,
        '7' | 'h' => Tool::Highlighter,
        'c' | 'b' => Tool::Obfuscate,
        'n' => Tool::Number,
        'f' => Tool::Focus,
        _ => return None,
    };
    Some((tool, tool_button_index(tool)))
}

pub fn constrained_drag_endpoint(
    tool: Tool,
    start: Point,
    end: Point,
    shift_pressed: bool,
) -> Point {
    if !shift_pressed {
        return end;
    }

    match tool {
        Tool::Line | Tool::Arrow => {
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            let max = dx.abs().max(dy.abs());

            let axis_threshold = 1.0 + std::f64::consts::SQRT_2;
            if dx.abs() >= dy.abs() * axis_threshold {
                Point {
                    x: end.x,
                    y: start.y,
                }
            } else if dy.abs() >= dx.abs() * axis_threshold {
                Point {
                    x: start.x,
                    y: end.y,
                }
            } else {
                Point {
                    x: start.x + max * dx.signum(),
                    y: start.y + max * dy.signum(),
                }
            }
        }
        Tool::Box | Tool::Circle => {
            let size = (end.x - start.x).abs().max((end.y - start.y).abs());
            Point {
                x: start.x + if end.x >= start.x { size } else { -size },
                y: start.y + if end.y >= start.y { size } else { -size },
            }
        }
        Tool::Highlighter => Point {
            x: end.x,
            y: start.y,
        },
        _ => end,
    }
}

pub fn cursor_name_for_select_handle(handle: SelectHandle) -> &'static str {
    match handle {
        SelectHandle::TopLeft => "nw-resize",
        SelectHandle::Top => "ns-resize",
        SelectHandle::TopRight => "ne-resize",
        SelectHandle::Left => "ew-resize",
        SelectHandle::Right => "ew-resize",
        SelectHandle::BottomLeft => "sw-resize",
        SelectHandle::Bottom => "ns-resize",
        SelectHandle::BottomRight => "se-resize",
        SelectHandle::Start | SelectHandle::End => "move",
    }
}
