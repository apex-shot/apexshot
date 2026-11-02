//! Card frame presets, shared by preview, export and inspector swatches.

use gtk4::cairo::Context;

use super::image_transform::StaticImageTransform;
use super::state::background::Layout;
use super::types::{DrawColor, FrameStyle};
use super::{render, transform_render};

#[derive(Clone, Copy)]
pub(crate) struct StyleGeometry {
    pub layout: Layout,
    pub width: f64,
    pub height: f64,
    pub pose: StaticImageTransform,
    pub style: FrameStyle,
    pub border_unit: f64,
    pub backing_unit: f64,
}

impl StyleGeometry {
    pub fn new(
        layout: Layout,
        width: f64,
        height: f64,
        pose: StaticImageTransform,
        style: FrameStyle,
    ) -> Self {
        Self {
            layout,
            width,
            height,
            pose,
            style,
            border_unit: width.max(height) / 400.0,
            backing_unit: 1.0,
        }
    }

    fn outline(&self, expand: f64) -> Vec<(f64, f64)> {
        let hw = (self.width / 2.0 + expand).max(0.01);
        let hh = (self.height / 2.0 + expand).max(0.01);
        let radius = if self.layout.radius <= 0.0 {
            0.0
        } else {
            (self.layout.radius + expand).clamp(0.0, hw.min(hh))
        };
        let project = transform_render::projector(self.layout, self.width, self.height, self.pose);
        if radius <= 0.01 {
            return [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                .into_iter()
                .map(|(x, y)| project(x, y))
                .collect();
        }
        let centers = [
            (hw - radius, -hh + radius, -std::f64::consts::FRAC_PI_2),
            (hw - radius, hh - radius, 0.0),
            (-hw + radius, hh - radius, std::f64::consts::FRAC_PI_2),
            (-hw + radius, -hh + radius, std::f64::consts::PI),
        ];
        let mut points = Vec::with_capacity(68);
        for (cx, cy, start) in centers {
            for step in 0..=16 {
                let angle = start + f64::from(step) / 16.0 * std::f64::consts::FRAC_PI_2;
                let (sin, cos) = angle.sin_cos();
                points.push(project(
                    cx + radius * cos.signum() * cos.abs().sqrt(),
                    cy + radius * sin.signum() * sin.abs().sqrt(),
                ));
            }
        }
        points
    }

    fn path(&self, context: &Context, expand: f64) {
        let points = self.outline(expand);
        context.new_sub_path();
        context.move_to(points[0].0, points[0].1);
        for point in &points[1..] {
            context.line_to(point.0, point.1);
        }
        context.close_path();
    }

    fn clip(&self, context: &Context) {
        context.rectangle(0.0, 0.0, self.layout.width, self.layout.height);
        context.clip();
    }

    fn band(&self, context: &Context, outer: f64, inner: f64, tint: DrawColor) {
        context.new_path();
        context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
        self.path(context, outer);
        self.path(context, inner);
        color(context, tint);
        context.fill().ok();
    }

    pub fn paint_underlay(&self, context: &Context) {
        let spec = self.style.spec();
        if spec.liquid || spec.inset_border || spec.border_thickness <= 0.0 {
            return;
        }
        let thickness = spec.border_thickness * self.border_unit;
        let _ = context.save();
        self.clip(context);
        self.band(context, thickness, -1.0, spec.border_color);
        context.restore().ok();
    }

    pub fn paint_backings(&self, context: &Context) {
        let spec = self.style.spec();
        if spec.backing1.is_none() && spec.backing2.is_none() {
            return;
        }
        let corners =
            transform_render::card_corners(self.layout, self.width, self.height, self.pose);
        let points = self.outline(0.0);
        let _ = context.save();
        self.clip(context);
        for backing in [spec.backing1, spec.backing2].into_iter().flatten() {
            let pivot = if backing.center_pivot {
                (
                    corners.iter().map(|point| point.0).sum::<f64>() / 4.0,
                    corners.iter().map(|point| point.1).sum::<f64>() / 4.0,
                )
            } else {
                corners[2]
            };
            let (sin, cos) = backing.rotation_deg.to_radians().sin_cos();
            context.new_sub_path();
            for (index, (x, y)) in points.iter().copied().enumerate() {
                let (x, y) = (x - pivot.0, y - pivot.1);
                let point = (
                    pivot.0 + backing.offset_x * self.backing_unit + x * cos - y * sin,
                    pivot.1 + backing.offset_y * self.backing_unit + x * sin + y * cos,
                );
                if index == 0 {
                    context.move_to(point.0, point.1);
                } else {
                    context.line_to(point.0, point.1);
                }
            }
            context.close_path();
            color(context, backing.color);
            context.fill().ok();
        }
        context.restore().ok();
    }

    pub fn paint_frame(&self, context: &Context) {
        if self.style == FrameStyle::Default {
            return;
        }
        let spec = self.style.spec();
        let _ = context.save();
        self.clip(context);
        if let Some(liquid) =
            render::LiquidFrame::resolve(&spec, spec.border_thickness, self.border_unit)
        {
            let corners =
                transform_render::card_corners(self.layout, self.width, self.height, self.pose);
            let top = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let bottom = corners
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max);
            liquid.paint(context, top, bottom, |context, expand| {
                self.path(context, expand)
            });
        }
        let mut expand = 0.0;
        if !spec.liquid && spec.border_thickness > 0.0 {
            let thickness = spec.border_thickness * self.border_unit;
            if spec.inset_border {
                self.band(context, 0.0, -thickness, spec.border_color);
                if self.style == FrameStyle::InsetLight {
                    self.band(
                        context,
                        -thickness,
                        -thickness - (0.75 * self.border_unit).max(0.3),
                        DrawColor::new(0.12, 0.14, 0.18, 0.35),
                    );
                }
            } else {
                expand = thickness;
            }
        }
        if !spec.liquid {
            for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
                expand += outer.gap * self.border_unit;
                let thickness = outer.thickness * self.border_unit;
                self.band(context, expand + thickness, expand, outer.color);
                expand += thickness;
            }
        }
        context.restore().ok();
    }
}

fn color(context: &Context, color: DrawColor) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
}

/// Match the shared composition's frame extents, including rotated sheets.
pub(crate) fn overhangs(style: FrameStyle, width: f64, height: f64) -> (f64, f64, f64, f64) {
    let spec = style.spec();
    let unit = width.max(height) / 400.0;
    let mut uniform = if spec.inset_border {
        0.0
    } else {
        spec.border_thickness * unit
    };
    for outer in [spec.outer1, spec.outer2].into_iter().flatten() {
        uniform += (outer.gap + outer.thickness) * unit;
    }
    let (mut left, mut top, mut right, mut bottom) = (uniform, uniform, uniform, uniform);
    for backing in [spec.backing1, spec.backing2].into_iter().flatten() {
        let (sin, cos) = backing.rotation_deg.to_radians().sin_cos();
        let (points, px, py) = if backing.center_pivot {
            let (hw, hh) = (width / 2.0, height / 2.0);
            (
                [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)],
                hw + backing.offset_x,
                hh + backing.offset_y,
            )
        } else {
            (
                [(-width, -height), (0.0, -height), (0.0, 0.0), (-width, 0.0)],
                width + backing.offset_x,
                height + backing.offset_y,
            )
        };
        for (x, y) in points {
            let x1 = px + x * cos - y * sin;
            let y1 = py + x * sin + y * cos;
            left = left.max(-x1);
            top = top.max(-y1);
            right = right.max(x1 - width);
            bottom = bottom.max(y1 - height);
        }
    }
    (left.max(0.0), top.max(0.0), right.max(0.0), bottom.max(0.0))
}
