//! Background composition shared by the Windows preview and export.

use std::sync::Arc;

use gtk4::cairo::{Context, Extend, Filter, Format, ImageSurface};
use image::RgbaImage;

use super::super::render::{self, blur_background_surface, paint_background_noise};
use crate::video_editor::background::FillPainter;

pub(crate) const DEFAULT_FILL_PADDING: f64 = 16.0;

#[derive(Clone)]
pub(crate) enum Fill {
    None,
    Image(Arc<RgbaImage>),
    Custom(FillPainter),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameRatio {
    pub width: u32,
    pub height: u32,
}

impl FrameRatio {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn aspect(self) -> f64 {
        f64::from(self.width) / f64::from(self.height)
    }
}

#[derive(Clone)]
pub(crate) struct Appearance {
    pub fill: Fill,
    pub padding: f64,
    pub blur: f64,
    pub radius: f64,
    pub noise: f64,
    pub frame_aspect: Option<FrameRatio>,
    pub frame_style: super::super::types::FrameStyle,
    pub revision: u64,
    padding_initialized: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            fill: Fill::None,
            padding: 0.0,
            blur: 0.0,
            radius: 0.0,
            noise: 0.0,
            frame_aspect: None,
            frame_style: Default::default(),
            revision: 0,
            padding_initialized: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    pub width: f64,
    pub height: f64,
    pub image_x: f64,
    pub image_y: f64,
    pub radius: f64,
}

impl Appearance {
    pub fn has_fill(&self) -> bool {
        !matches!(self.fill, Fill::None)
    }

    pub fn set_fill(&mut self, fill: Fill) {
        if !matches!(fill, Fill::None) && !self.padding_initialized {
            self.padding = DEFAULT_FILL_PADDING;
            self.padding_initialized = true;
        }
        self.fill = fill;
    }

    pub fn set_padding(&mut self, padding: f64) {
        self.padding = padding;
        self.padding_initialized = true;
    }

    pub fn layout(&self, width: u32, height: u32) -> Layout {
        let unit = f64::from(width.max(height)) / 400.0;
        let inset = if self.has_fill() {
            (self.padding.max(0.0) * unit).ceil()
        } else {
            0.0
        };
        let mut canvas_width = f64::from(width) + 2.0 * inset;
        let mut canvas_height = f64::from(height) + 2.0 * inset;
        let (left, top, right, bottom) = super::super::style_render::overhangs(
            self.frame_style,
            f64::from(width),
            f64::from(height),
        );
        let (left, top, right, bottom) = (left.ceil(), top.ceil(), right.ceil(), bottom.ceil());
        let content_width = f64::from(width) + left + right;
        let content_height = f64::from(height) + top + bottom;
        canvas_width = canvas_width.max(content_width.ceil());
        canvas_height = canvas_height.max(content_height.ceil());
        if let Some(ratio) = self.frame_aspect.filter(|r| r.width > 0 && r.height > 0) {
            let w = f64::from(ratio.width);
            let h = f64::from(ratio.height);
            let multiple = (canvas_width / w).max(canvas_height / h).ceil();
            canvas_width = multiple * w;
            canvas_height = multiple * h;
        }
        Layout {
            width: canvas_width,
            height: canvas_height,
            image_x: ((canvas_width - content_width) / 2.0).floor() + left,
            image_y: ((canvas_height - content_height) / 2.0).floor() + top,
            radius: self.radius * unit,
        }
    }

    pub fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Rasterize only the fill, at a bounded size for the live preview.
    pub fn surface(&self, layout: Layout, max_edge: f64) -> Option<ImageSurface> {
        if !self.has_fill() {
            return None;
        }
        let scale = (max_edge / layout.width.max(layout.height)).min(1.0);
        let width = (layout.width * scale).round().max(1.0) as i32;
        let height = (layout.height * scale).round().max(1.0) as i32;
        let surface = ImageSurface::create(Format::ARgb32, width, height).ok()?;
        {
            let context = Context::new(&surface).ok()?;
            match &self.fill {
                Fill::None => return None,
                Fill::Custom(paint) => paint(&context, width, height),
                Fill::Image(image) => {
                    let source = render::rgba_image_to_surface(image)?;
                    let cover = (f64::from(width) / f64::from(image.width()))
                        .max(f64::from(height) / f64::from(image.height()));
                    context.translate(
                        (f64::from(width) - f64::from(image.width()) * cover) / 2.0,
                        (f64::from(height) - f64::from(image.height()) * cover) / 2.0,
                    );
                    context.scale(cover, cover);
                    context.set_source_surface(&source, 0.0, 0.0).ok()?;
                    context.source().set_filter(Filter::Bilinear);
                    context.source().set_extend(Extend::Pad);
                    context.paint().ok()?;
                }
            }
        }
        let surface =
            blur_background_surface(&surface, self.blur, layout.width, max_edge).unwrap_or(surface);
        if self.noise > 0.0 {
            let context = Context::new(&surface).ok()?;
            paint_background_noise(
                &context,
                0.0,
                0.0,
                f64::from(width),
                f64::from(height),
                self.noise,
            );
        }
        Some(surface)
    }
}

#[cfg(test)]
mod tests {
    use super::{Appearance, Fill, DEFAULT_FILL_PADDING};
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn image_fill() -> Fill {
        Fill::Image(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))))
    }

    #[test]
    fn first_fill_adds_editable_padding_without_overriding_later_choices() {
        let mut appearance = Appearance::default();
        assert_eq!(appearance.padding, 0.0);
        appearance.set_fill(image_fill());
        assert_eq!(appearance.padding, DEFAULT_FILL_PADDING);
        assert!(appearance.layout(400, 300).image_x > 0.0);
        appearance.set_padding(0.0);
        assert_eq!(appearance.layout(400, 300).image_x, 0.0);
        appearance.set_fill(image_fill());
        assert_eq!(appearance.padding, 0.0);
        appearance.set_fill(Fill::None);
        appearance.set_fill(image_fill());
        assert_eq!(appearance.padding, 0.0);
    }

    #[test]
    fn padding_edited_before_first_fill_is_preserved() {
        let mut appearance = Appearance::default();
        appearance.set_padding(5.0);
        appearance.set_fill(image_fill());
        assert_eq!(appearance.padding, 5.0);
    }
}
