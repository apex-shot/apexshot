use super::numbering_style::{NumberSize, NumberingStyle};
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
    Gradient(usize),
    Wallpaper(PathBuf),
    Blurred(usize),
    PlainColor(DrawColor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    BlurSecure,
    BlurSmooth,
    Blackout,
}

#[allow(dead_code)]
impl ObfuscateMethod {
    pub fn display_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "Pixelate",
            ObfuscateMethod::BlurSecure => "Blur (Secure)",
            ObfuscateMethod::BlurSmooth => "Blur (Smooth)",
            ObfuscateMethod::Blackout => "Blackout",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "obfuscate-pixelate",
            ObfuscateMethod::BlurSecure => "obfuscate-blur-secure",
            ObfuscateMethod::BlurSmooth => "obfuscate-blur-smooth",
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

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Standard => "go-next-symbolic",
            Self::Fancy => "go-next-symbolic",
            Self::Curved => "path-bezier-symbolic",
            Self::Double => "object-flip-horizontal-symbolic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropAspectRatio {
    Freeform,
    Original,
    Square,
    FourThree,
    SixteenNine,
    TwentyOneNine,
    ThreeTwo,
    NineSixteen,
}

impl CropAspectRatio {
    pub const ALL: [Self; 8] = [
        Self::Freeform,
        Self::Original,
        Self::Square,
        Self::FourThree,
        Self::SixteenNine,
        Self::TwentyOneNine,
        Self::ThreeTwo,
        Self::NineSixteen,
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
}

impl ViewTransform {
    pub fn for_image(image_width: f64, image_height: f64) -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            image_width,
            image_height,
        }
    }

    pub fn fit(image_width: f64, image_height: f64, view_width: f64, view_height: f64) -> Self {
        if image_width <= 0.0 || image_height <= 0.0 || view_width <= 1.0 || view_height <= 1.0 {
            return Self::for_image(image_width.max(1.0), image_height.max(1.0));
        }

        let scale = (view_width / image_width)
            .min(view_height / image_height)
            .min(1.0);

        let draw_width = image_width * scale;
        let draw_height = image_height * scale;

        Self {
            scale,
            offset_x: (view_width - draw_width) / 2.0,
            offset_y: (view_height - draw_height) / 2.0,
            image_width,
            image_height,
        }
    }

    pub fn contains_view(&self, point: Point) -> bool {
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
    },
    Line {
        start: Point,
        end: Point,
        color: DrawColor,
        stroke_size: f64,
    },
    Arrow {
        start: Point,
        end: Point,
        color: DrawColor,
        stroke_size: f64,
        style: ArrowStyle,
        control_points: Option<[Point; 3]>,
    },
    Box {
        rect: Rect,
        color: DrawColor,
        stroke_size: f64,
    },
    #[allow(dead_code)]
    Text {
        position: Point,
        text: String,
        color: DrawColor,
        font: FontSettings,
        max_width: Option<f64>,
    },
    Number {
        position: Point,
        number: u32,
        color: DrawColor,
        style: NumberingStyle,
        size: NumberSize,
    },
    Obfuscate {
        rect: Rect,
        method: ObfuscateMethod,
        amount: f64,
    },
    Focus {
        rect: Rect,
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

pub fn tool_shortcut_target(key: char) -> Option<(Tool, usize)> {
    match key.to_ascii_lowercase() {
        '0' | '`' | 's' => Some((Tool::Select, 2)),
        '1' | 'd' | 'p' => Some((Tool::Pen, 3)),
        '2' | 't' => Some((Tool::Text, 8)),
        '3' | 'l' => Some((Tool::Line, 7)),
        '4' | 'a' => Some((Tool::Arrow, 6)),
        '5' | 'r' => Some((Tool::Box, 4)),
        '6' | 'o' => Some((Tool::Circle, 5)),
        '7' | 'h' => Some((Tool::Highlighter, 12)),
        'c' | 'C' => Some((Tool::Obfuscate, 9)),
        'n' | 'N' => Some((Tool::Number, 11)),
        'x' | 'X' => Some((Tool::Crop, 0)),
        'b' | 'B' => Some((Tool::Obfuscate, 9)),
        'f' | 'F' => Some((Tool::Focus, 10)),
        _ => None,
    }
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
            if (end.x - start.x).abs() >= (end.y - start.y).abs() {
                Point {
                    x: end.x,
                    y: start.y,
                }
            } else {
                Point {
                    x: start.x,
                    y: end.y,
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
