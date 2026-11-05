//! The packaging mark shown above every step's page.

use std::f64::consts::{FRAC_PI_2, PI};

use gtk4::prelude::*;
use gtk4::{Align, DrawingArea};

use super::px;

/// The packaging mark, drawn on its original 128-unit grid rather than
/// loading an image or depending on an installed icon theme.
pub fn brand_mark(scale: f64, _dark: bool) -> DrawingArea {
    let size = px(scale, 40.0);
    let mark = DrawingArea::new();
    mark.add_css_class("onboarding-brand");
    mark.set_content_width(size);
    mark.set_content_height(size);
    mark.set_halign(Align::Center);
    mark.set_can_target(false);
    mark.set_draw_func(move |_, cr, width, _| {
        let u = f64::from(width) / 128.0;
        let (x, y, w, r) = (8.0 * u, 8.0 * u, 112.0 * u, 24.0 * u);
        cr.new_sub_path();
        cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
        cr.arc(x + w - r, y + w - r, r, 0.0, FRAC_PI_2);
        cr.arc(x + r, y + w - r, r, FRAC_PI_2, PI);
        cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
        cr.close_path();
        cr.set_source_rgb(0.914, 0.329, 0.125);
        let _ = cr.fill();
        cr.move_to(24.0 * u, 96.0 * u);
        cr.curve_to(40.0 * u, 96.0 * u, 48.0 * u, 20.0 * u, 64.0 * u, 20.0 * u);
        cr.curve_to(80.0 * u, 20.0 * u, 88.0 * u, 96.0 * u, 104.0 * u, 96.0 * u);
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(14.0 * u);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        let _ = cr.stroke();
    });
    mark
}
