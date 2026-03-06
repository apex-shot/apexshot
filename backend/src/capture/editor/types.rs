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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Pen,
    Highlighter,
    Circle,
    Arrow,
    Line,
    Box,
    Text,
    Number,
    Blur,
    Focus,
    Censor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeControlMode {
    Stroke,
    Text,
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

    pub fn view_to_image_clamped(&self, point: Point) -> Point {
        let scale = self.scale.max(0.0001);
        let mut ix = (point.x - self.offset_x) / scale;
        let mut iy = (point.y - self.offset_y) / scale;

        ix = ix.clamp(0.0, self.image_width);
        iy = iy.clamp(0.0, self.image_height);

        Point { x: ix, y: iy }
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
    },
    Box {
        rect: Rect,
        color: DrawColor,
        stroke_size: f64,
    },
    Text {
        position: Point,
        text: String,
        color: DrawColor,
        font_size: f64,
    },
    Number {
        position: Point,
        number: u32,
        color: DrawColor,
    },
    Blur {
        rect: Rect,
    },
    Focus {
        rect: Rect,
    },
    Censor {
        rect: Rect,
    },
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
        '0' | '`' | 's' => Some((Tool::Select, 0)),
        '1' | 'd' | 'p' => Some((Tool::Pen, 2)),
        '2' | 't' => Some((Tool::Text, 7)),
        '3' | 'l' => Some((Tool::Line, 6)),
        '4' | 'a' => Some((Tool::Arrow, 5)),
        '5' | 'r' => Some((Tool::Box, 3)),
        '6' | 'o' => Some((Tool::Circle, 4)),
        '7' | 'h' => Some((Tool::Highlighter, 11)),
        '8' | 'c' => Some((Tool::Censor, 9)),
        '9' | 'n' => Some((Tool::Number, 10)),
        'x' => Some((Tool::Crop, 1)),
        'b' => Some((Tool::Blur, 8)),
        'f' => Some((Tool::Focus, 12)),
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
