//! The welcome brand and small, page-specific onboarding illustrations.

use std::f64::consts::{FRAC_PI_2, PI};

use gtk4::prelude::*;
use gtk4::{cairo, Align, DrawingArea};

use super::{px, OnboardingStep};

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

/// Keep the welcome brand; give later pages a compact visual of their subject
/// in the same header slot, without adding copy or another content panel.
pub fn step_mark(step: OnboardingStep, scale: f64, dark: bool) -> DrawingArea {
    if step == OnboardingStep::Welcome {
        return brand_mark(scale, dark);
    }
    let mark = DrawingArea::new();
    mark.add_css_class("onboarding-illustration");
    mark.set_content_width(px(scale, 96.0));
    mark.set_content_height(px(scale, 52.0));
    mark.set_halign(Align::Center);
    mark.set_can_target(false);
    mark.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let ink = Ink {
            foreground: (
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
            ),
            background: if dark {
                (25.0 / 255.0, 25.0 / 255.0, 25.0 / 255.0)
            } else {
                (244.0 / 255.0, 242.0 / 255.0, 238.0 / 255.0)
            },
        };
        cr.scale(f64::from(width) / 96.0, f64::from(height) / 52.0);
        paint(cr, step, ink);
    });
    mark
}

#[derive(Clone, Copy)]
struct Ink {
    foreground: (f64, f64, f64),
    background: (f64, f64, f64),
}

impl Ink {
    fn stroke(self, cr: &cairo::Context, alpha: f64) {
        let (r, g, b) = self.foreground;
        cr.set_source_rgba(r, g, b, alpha);
    }

    fn fill(self, cr: &cairo::Context) {
        let (r, g, b) = self.background;
        let (fr, fg, fb) = self.foreground;
        cr.set_source_rgb(
            r + (fr - r) * 0.055,
            g + (fg - g) * 0.055,
            b + (fb - b) * 0.055,
        );
    }
}

fn paint(cr: &cairo::Context, step: OnboardingStep, ink: Ink) {
    cr.set_line_width(1.3);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match step {
        OnboardingStep::HowToUse => keycaps(cr, ink),
        OnboardingStep::Cloud => cloud(cr, ink),
        OnboardingStep::Complete => complete(cr, ink),
        OnboardingStep::Welcome => {}
    }
}

fn rounded_rect(cr: &cairo::Context, (x, y, w, h): (f64, f64, f64, f64), r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

fn fill_and_outline(cr: &cairo::Context, ink: Ink) {
    ink.fill(cr);
    let _ = cr.fill_preserve();
    ink.stroke(cr, 0.30);
    let _ = cr.stroke();
}

fn keycaps(cr: &cairo::Context, ink: Ink) {
    let _ = cr.save();
    cr.translate(31.0, 24.0);
    cr.rotate(-0.14);
    rounded_rect(cr, (-15.0, -15.0, 30.0, 30.0), 6.0);
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.72);
    cr.move_to(0.0, -7.0);
    cr.line_to(-6.0, -1.0);
    cr.line_to(-2.5, -1.0);
    cr.line_to(-2.5, 6.0);
    cr.line_to(2.5, 6.0);
    cr.line_to(2.5, -1.0);
    cr.line_to(6.0, -1.0);
    cr.close_path();
    let _ = cr.stroke();
    let _ = cr.restore();

    let _ = cr.save();
    cr.translate(59.0, 29.0);
    cr.rotate(0.10);
    rounded_rect(cr, (-16.0, -15.0, 32.0, 30.0), 6.0);
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.82);
    cr.move_to(6.0, -5.0);
    cr.line_to(6.0, 1.0);
    cr.line_to(-6.0, 1.0);
    cr.move_to(-2.0, -3.0);
    cr.line_to(-6.0, 1.0);
    cr.line_to(-2.0, 5.0);
    let _ = cr.stroke();
    let _ = cr.restore();

    cr.move_to(75.0, 33.0);
    cr.line_to(76.0, 46.0);
    cr.line_to(80.0, 42.0);
    cr.line_to(85.0, 41.0);
    cr.close_path();
    fill_and_outline(cr, ink);
}

fn cloud(cr: &cairo::Context, ink: Ink) {
    let _ = cr.save();
    cr.translate(34.0, 26.0);
    cr.rotate(-0.16);
    rounded_rect(cr, (-14.0, -17.0, 28.0, 34.0), 4.0);
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.35);
    cr.move_to(-7.0, -9.0);
    cr.line_to(4.0, -9.0);
    cr.move_to(-7.0, -3.0);
    cr.line_to(1.0, -3.0);
    let _ = cr.stroke();
    let _ = cr.restore();

    cr.move_to(39.0, 43.0);
    cr.line_to(68.0, 43.0);
    cr.curve_to(83.0, 43.0, 85.0, 27.0, 73.0, 25.0);
    cr.curve_to(72.0, 13.0, 60.0, 8.0, 51.0, 17.0);
    cr.curve_to(42.0, 12.0, 32.0, 19.0, 34.0, 28.0);
    cr.curve_to(22.0, 28.0, 25.0, 43.0, 39.0, 43.0);
    cr.close_path();
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.82);
    cr.move_to(54.0, 36.0);
    cr.line_to(54.0, 24.0);
    cr.move_to(49.0, 29.0);
    cr.line_to(54.0, 24.0);
    cr.line_to(59.0, 29.0);
    let _ = cr.stroke();
}

fn complete(cr: &cairo::Context, ink: Ink) {
    let _ = cr.save();
    cr.translate(42.0, 25.0);
    cr.rotate(-0.14);
    rounded_rect(cr, (-14.0, -17.0, 28.0, 34.0), 4.0);
    fill_and_outline(cr, ink);
    let _ = cr.restore();
    rounded_rect(cr, (35.0, 13.0, 28.0, 34.0), 4.0);
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.38);
    cr.move_to(42.0, 21.0);
    cr.line_to(54.0, 21.0);
    cr.move_to(42.0, 27.0);
    cr.line_to(49.0, 27.0);
    let _ = cr.stroke();

    cr.arc(65.0, 34.0, 12.0, 0.0, 2.0 * PI);
    fill_and_outline(cr, ink);
    ink.stroke(cr, 0.88);
    cr.set_line_width(2.0);
    cr.move_to(59.0, 34.0);
    cr.line_to(63.0, 38.0);
    cr.line_to(71.0, 29.0);
    let _ = cr.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn illustrations_are_distinct_neutral_and_contained() {
        for dark in [true, false] {
            let ink = Ink {
                foreground: if dark {
                    (0.95, 0.94, 0.93)
                } else {
                    (0.15, 0.14, 0.12)
                },
                background: if dark {
                    (0.10, 0.10, 0.10)
                } else {
                    (0.96, 0.95, 0.93)
                },
            };
            let mut images = Vec::new();
            for step in [
                OnboardingStep::HowToUse,
                OnboardingStep::Cloud,
                OnboardingStep::Complete,
            ] {
                let mut surface =
                    cairo::ImageSurface::create(cairo::Format::ARgb32, 96, 52).unwrap();
                {
                    let cr = cairo::Context::new(&surface).unwrap();
                    paint(&cr, step, ink);
                    cr.status().unwrap();
                }
                let pixels = surface.data().unwrap().to_vec();
                let (rgba, _) = pixels.as_chunks::<4>();
                assert!(rgba.iter().any(|pixel| pixel[3] > 0));
                for (index, pixel) in rgba.iter().enumerate() {
                    let max = *pixel[..3].iter().max().unwrap();
                    let min = *pixel[..3].iter().min().unwrap();
                    assert!(max - min <= 12, "illustration introduced a color accent");
                    let (x, y) = (index % 96, index / 96);
                    if x == 0 || x == 95 || y == 0 || y == 51 {
                        assert_eq!(pixel[3], 0, "illustration touched its bounds");
                    }
                }
                images.push(pixels);
            }
            assert_ne!(images[0], images[1]);
            assert_ne!(images[1], images[2]);
            assert_ne!(images[0], images[2]);
        }
    }

    #[test]
    fn decorative_marks_stay_compact_at_each_display_scale() {
        let _ = crate::test_support::with_gtk(|| {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for step in OnboardingStep::all() {
                    let mark = step_mark(step, scale, true);
                    let (width, height) = if step == OnboardingStep::Welcome {
                        (40.0, 40.0)
                    } else {
                        (96.0, 52.0)
                    };
                    assert_eq!(mark.content_width(), px(scale, width));
                    assert_eq!(mark.content_height(), px(scale, height));
                    assert!(!mark.can_target());
                    assert!(!mark.is_focusable());
                    assert!(mark.tooltip_text().is_none());
                }
            }
        });
    }
}
