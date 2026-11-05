//! Still-image glass frames use the shared refractive renderer.

use gtk4::cairo::{Context, Filter, ImageSurface};
use image::RgbaImage;

use super::render::{self, GlassLook, GlassRing};
use super::state::background::{Appearance, Layout};

#[derive(Clone)]
pub(crate) struct GlassOverlay {
    surface: ImageSurface,
    x: f64,
    y: f64,
    scale_x: f64,
    scale_y: f64,
}

impl GlassOverlay {
    pub fn paint(&self, context: &Context) {
        let _ = context.save();
        context.translate(self.x, self.y);
        context.scale(1.0 / self.scale_x, 1.0 / self.scale_y);
        context.set_source_surface(&self.surface, 0.0, 0.0).ok();
        context.source().set_filter(Filter::Bilinear);
        context.paint().ok();
        context.restore().ok();
    }

    #[cfg(test)]
    pub fn same_surface(&self, other: &Self) -> bool {
        self.surface.to_raw_none() == other.surface.to_raw_none()
    }
}

pub(crate) fn render(
    appearance: &Appearance,
    layout: Layout,
    image_width: u32,
    image_height: u32,
    max_edge: f64,
) -> Option<GlassOverlay> {
    let spec = appearance.frame_style.spec();
    if !spec.liquid {
        return None;
    }
    let factor = (max_edge / layout.width.max(layout.height)).min(1.0);
    let width = (layout.width * factor).round().max(1.0) as u32;
    let height = (layout.height * factor).round().max(1.0) as u32;
    let backdrop = if appearance.has_fill() {
        let mut surface = appearance.surface(layout, max_edge)?;
        surface.flush();
        let stride = surface.stride() as usize;
        let pixels = surface.data().ok()?;
        render::cairo_argb_to_rgba_image(width, height, stride, pixels.as_ref())
    } else {
        RgbaImage::new(width, height)
    };
    let scale_x = f64::from(width) / layout.width;
    let scale_y = f64::from(height) / layout.height;
    let unit = f64::from(image_width.max(image_height)) / 400.0;
    let ring = GlassRing {
        x: layout.image_x * scale_x,
        y: layout.image_y * scale_y,
        width: f64::from(image_width) * scale_x,
        height: f64::from(image_height) * scale_y,
        radius: layout.radius * scale_x.min(scale_y),
        gap: (spec.border_thickness + spec.outer1.map_or(0.0, |outer| outer.thickness))
            * unit
            * scale_x.min(scale_y),
    };
    let layer = render::glass_layer(
        &backdrop,
        &ring,
        &GlassLook::for_style(appearance.frame_style),
    )?;
    Some(GlassOverlay {
        surface: render::rgba_image_to_surface(&layer.image)?,
        x: layer.x as f64 / scale_x,
        y: layer.y as f64 / scale_y,
        scale_x,
        scale_y,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::capture::editor::state::background::Fill;
    use crate::capture::editor::types::FrameStyle;

    #[test]
    fn glass_uses_the_shared_refraction_on_the_background() {
        let mut appearance = Appearance::default();
        appearance.padding = 60.0;
        appearance.radius = 20.0;
        appearance.frame_style = FrameStyle::Liquid;
        appearance.fill = Fill::Image(Arc::new(RgbaImage::from_fn(12, 12, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgba([235, 80, 40, 255])
            } else {
                image::Rgba([20, 120, 210, 255])
            }
        })));
        let layout = appearance.layout(400, 300);
        let edge = layout.width.max(layout.height);
        let mut background = appearance.surface(layout, edge).unwrap();
        background.flush();
        let stride = background.stride() as usize;
        let backdrop = {
            let data = background.data().unwrap();
            render::cairo_argb_to_rgba_image(
                layout.width as u32,
                layout.height as u32,
                stride,
                data.as_ref(),
            )
        };
        let spec = appearance.frame_style.spec();
        let ring = GlassRing {
            x: layout.image_x,
            y: layout.image_y,
            width: 400.0,
            height: 300.0,
            radius: layout.radius,
            gap: spec.border_thickness + spec.outer1.unwrap().thickness,
        };
        let expected =
            render::glass_layer(&backdrop, &ring, &GlassLook::for_style(FrameStyle::Liquid))
                .expect("glass ring should be visible");
        let mut actual = render(&appearance, layout, 400, 300, edge)
            .expect("glass overlay should be visible")
            .surface;
        actual.flush();
        let stride = actual.stride() as usize;
        let width = actual.width() as u32;
        let height = actual.height() as u32;
        let data = actual.data().unwrap();
        let actual = render::cairo_argb_to_rgba_image(width, height, stride, data.as_ref());
        assert_eq!(actual.dimensions(), expected.image.dimensions());
        // Compare premultiplied channels: straight color is unstable at low alpha.
        for (a, b) in actual.pixels().zip(expected.image.pixels()) {
            assert!(a[3].abs_diff(b[3]) <= 1);
            for channel in 0..3 {
                let premultiply =
                    |value: u8, alpha: u8| (u16::from(value) * u16::from(alpha) / 255) as u8;
                assert!(premultiply(a[channel], a[3]).abs_diff(premultiply(b[channel], b[3])) <= 1);
            }
        }
    }
}
