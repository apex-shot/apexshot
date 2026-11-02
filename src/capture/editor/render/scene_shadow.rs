//! Blurred card silhouette shared by the still and Motion compositors.

use gtk4::cairo::{Context, Filter, Format, ImageSurface};

use super::{apply_blur_rect, cairo_argb_to_rgba_image, rgba_image_to_surface};
use crate::capture::editor::types::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneShadowGeometry {
    pub bounds: (f64, f64, f64, f64),
    pub corners: [(f64, f64); 4],
    pub radius: f64,
    pub opacity: f64,
    pub blur: f64,
    pub offset: (f64, f64),
}

/// Build a bounded-resolution alpha mask so previews can cache it separately
/// from the background and card textures.
pub(crate) fn shadow_mask(geometry: SceneShadowGeometry, max_edge: f64) -> Option<ImageSurface> {
    let (x, y, width, height) = geometry.bounds;
    let opacity = geometry.opacity.clamp(0.0, 1.0);
    if opacity <= 0.001 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let scale = (max_edge / width.max(height)).min(1.0);
    let mask_w = (width * scale).ceil().max(1.0) as i32;
    let mask_h = (height * scale).ceil().max(1.0) as i32;
    let mut mask = ImageSurface::create(Format::ARgb32, mask_w, mask_h).ok()?;
    {
        let context = Context::new(&mask).ok()?;
        context.set_source_rgba(0.0, 0.0, 0.0, opacity);
        trace_rounded_quad(&context, geometry, scale, (x, y));
        context.fill().ok()?;
    }
    if geometry.blur < 0.5 {
        return Some(mask);
    }
    mask.flush();
    let stride = mask.stride() as usize;
    let mut rgba = {
        let data = mask.data().ok()?;
        cairo_argb_to_rgba_image(mask_w as u32, mask_h as u32, stride, data.as_ref())
    };
    let rect = Rect {
        x: 0,
        y: 0,
        width: mask_w,
        height: mask_h,
    };
    let sigma = (geometry.blur * scale * 0.4).max(0.1);
    for _ in 0..3 {
        apply_blur_rect(&mut rgba, rect, sigma, true);
    }
    rgba_image_to_surface(&rgba)
}

pub(crate) fn paint_shadow_mask(
    context: &Context,
    surface: &ImageSurface,
    bounds: (f64, f64, f64, f64),
    opacity: f64,
) {
    let (x, y, width, height) = bounds;
    let _ = context.save();
    context.rectangle(x, y, width, height);
    context.clip();
    context.translate(x, y);
    context.scale(
        width / f64::from(surface.width()),
        height / f64::from(surface.height()),
    );
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(Filter::Bilinear);
    if opacity >= 0.999 {
        context.paint().ok();
    } else {
        context.paint_with_alpha(opacity.clamp(0.0, 1.0)).ok();
    }
    context.restore().ok();
}

pub(crate) fn paint_shadow(context: &Context, geometry: SceneShadowGeometry, interactive: bool) {
    if geometry.opacity.clamp(0.0, 1.0) <= 0.001 {
        return;
    }
    if geometry.blur < 0.5 {
        let (x, y, width, height) = geometry.bounds;
        let _ = context.save();
        context.rectangle(x, y, width, height);
        context.clip();
        context.set_source_rgba(0.0, 0.0, 0.0, geometry.opacity.clamp(0.0, 1.0));
        trace_rounded_quad(context, geometry, 1.0, (0.0, 0.0));
        context.fill().ok();
        context.restore().ok();
    } else if let Some(surface) = shadow_mask(geometry, if interactive { 320.0 } else { 720.0 }) {
        paint_shadow_mask(context, &surface, geometry.bounds, 1.0);
    }
}

fn trace_rounded_quad(
    context: &Context,
    geometry: SceneShadowGeometry,
    scale: f64,
    origin: (f64, f64),
) {
    let points = geometry.corners.map(|(x, y)| {
        (
            (x + geometry.offset.0 - origin.0) * scale,
            (y + geometry.offset.1 - origin.1) * scale,
        )
    });
    let mut shortest = f64::INFINITY;
    for index in 0..4 {
        let a = points[index];
        let b = points[(index + 1) % 4];
        shortest = shortest.min((b.0 - a.0).hypot(b.1 - a.1));
    }
    let radius = (geometry.radius * scale).clamp(0.0, shortest / 2.0);
    if radius <= 0.5 {
        context.move_to(points[0].0, points[0].1);
        for point in &points[1..] {
            context.line_to(point.0, point.1);
        }
        context.close_path();
        return;
    }
    let direction = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy).max(1e-6);
        (dx / length, dy / length)
    };
    for index in 0..4 {
        let previous = points[(index + 3) % 4];
        let corner = points[index];
        let next = points[(index + 1) % 4];
        let incoming = direction(previous, corner);
        let outgoing = direction(corner, next);
        let start = (
            corner.0 - incoming.0 * radius,
            corner.1 - incoming.1 * radius,
        );
        let end = (
            corner.0 + outgoing.0 * radius,
            corner.1 + outgoing.1 * radius,
        );
        if index == 0 {
            context.move_to(start.0, start.1);
        } else {
            context.line_to(start.0, start.1);
        }
        let control_1 = (
            start.0 + (corner.0 - start.0) * (2.0 / 3.0),
            start.1 + (corner.1 - start.1) * (2.0 / 3.0),
        );
        let control_2 = (
            end.0 + (corner.0 - end.0) * (2.0 / 3.0),
            end.1 + (corner.1 - end.1) * (2.0 / 3.0),
        );
        context.curve_to(
            control_1.0,
            control_1.1,
            control_2.0,
            control_2.1,
            end.0,
            end.1,
        );
    }
    context.close_path();
}
