use gtk4::cairo::LineCap;
use gtk4::{gdk, prelude::*, Align, Box as GtkBox, DrawingArea, Label, Orientation};

use crate::i18n::t;
use crate::recording::editor::model::{
    MotionEffectTransformTiming, MotionTimingKind, MAX_MOTION_POS, MAX_MOTION_YAW, MIN_MOTION_POS,
    MIN_MOTION_YAW,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

pub(super) fn motion_rgba([red, green, blue, alpha]: [f64; 4]) -> gdk::RGBA {
    gdk::RGBA::new(red as f32, green as f32, blue as f32, alpha as f32)
}

/// The 8-bit triple the shared color picker speaks, dropping alpha.
pub(super) fn motion_rgb888(rgba: gdk::RGBA) -> (u8, u8, u8) {
    let component = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    (
        component(rgba.red()),
        component(rgba.green()),
        component(rgba.blue()),
    )
}

pub(super) fn motion_appearance_slider(
    title: &str,
    min: f64,
    max: f64,
    value: f64,
    suffix: &'static str,
) -> FillSlider {
    let slider = FillSlider::new_with_value_text(&t(title), move |value, _, _| {
        if suffix == "%" {
            format!("{:.0}%", value * 100.0)
        } else {
            format!("{value:.0}{suffix}")
        }
    });
    slider.set_range(min, max);
    slider.set_increments((max - min) / 100.0, (max - min) / 10.0);
    slider.set_value(value);
    slider
}

/// Slider for values stored as "reference px against a 400px long edge"
/// (padding, corner radius). The stored unit is a fraction of the long edge,
/// so the honest readout is a percentage of it: a stored 40 renders as 10% per
/// side on any image size. Labelling these values "px" reads as literal pixels,
/// which is wrong for every image whose long edge is not exactly 400px.
pub(super) fn motion_reference_percent_slider(
    title: &str,
    min: f64,
    max: f64,
    value: f64,
) -> FillSlider {
    let slider =
        FillSlider::new_with_value_text(&t(title), |value, _, _| format!("{:.0}%", value / 4.0));
    slider.set_range(min, max);
    slider.set_increments((max - min) / 100.0, (max - min) / 10.0);
    slider.set_value(value);
    slider
}

pub(super) fn format_duration_label(duration: f64) -> String {
    format!("{duration:.1}s")
}

pub(super) fn span_slider_row(
    title: &str,
    initial: f64,
    min: f64,
    max: f64,
) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{:.0}%", initial * 100.0)));
    header.set_visible(false);
    let slider =
        FillSlider::new_with_value_text(title, |value, _, _| format!("{:.0}%", value * 100.0));
    slider.set_range(min, max);
    slider.set_increments(0.01, 0.1);
    slider.set_value(initial);
    (header, value, slider)
}

/// Motion position is stored as a normalized value for resolution-independent
/// rendering, but the inspector mirrors the direct-manipulation control in
/// whole 3D position units (for example, `-31` and `303`).
pub(super) fn position_slider_row(title: &str, initial: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{:.0}", initial * 1000.0)));
    header.set_visible(false);
    let slider =
        FillSlider::new_with_value_text(title, |value, _, _| format!("{:.0}", value * 1000.0));
    slider.set_range(MIN_MOTION_POS, MAX_MOTION_POS);
    // One visible position unit per drag increment.
    slider.set_increments(0.001, 0.1);
    slider.set_value(initial);
    (header, value, slider)
}

pub(super) fn angle_slider_row(title: &str, initial: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{initial:.0}°")));
    header.set_visible(false);
    let slider = FillSlider::new_with_value_text(title, |value, _, _| format!("{value:.0}°"));
    slider.set_range(MIN_MOTION_YAW, MAX_MOTION_YAW);
    slider.set_increments(1.0, 5.0);
    slider.set_value(initial);
    (header, value, slider)
}

/// Named cubic-Bézier presets for the Ease timing mode. Snappy is the
/// default curve; Smooth is the neutral S; Linear matches constant motion.
pub(super) const EASE_PRESETS: [(&str, f64, f64, f64, f64); 3] = [
    ("Smooth", 0.42, 0.0, 0.58, 1.0),
    ("Snappy", 0.25, 1.0, 0.50, 1.0),
    ("Linear", 0.0, 0.0, 1.0, 1.0),
];

/// Named spring presets as first-peak overshoot fractions: Smooth never
/// passes the target, Gentle (the default) rounds off softly, and Bouncy is
/// the playful one. The Bounce slider covers everything in between.
pub(super) const SPRING_PRESETS: [(&str, f64); 3] =
    [("Smooth", 0.0), ("Gentle", 0.05), ("Bouncy", 0.35)];

pub(super) fn ease_preset_timing(
    index: usize,
    base: MotionEffectTransformTiming,
) -> MotionEffectTransformTiming {
    let (_, x1, y1, x2, y2) = EASE_PRESETS[index];
    MotionEffectTransformTiming {
        easing_x1: x1,
        easing_y1: y1,
        easing_x2: x2,
        easing_y2: y2,
        ..base
    }
}

pub(super) fn spring_preset_timing(
    index: usize,
    base: MotionEffectTransformTiming,
) -> MotionEffectTransformTiming {
    MotionEffectTransformTiming {
        kind: MotionTimingKind::Spring,
        spring_bounce: SPRING_PRESETS[index].1,
        ..base
    }
}

#[allow(dead_code)]
pub(super) fn matching_ease_preset(timing: MotionEffectTransformTiming) -> Option<usize> {
    EASE_PRESETS.iter().position(|(_, x1, y1, x2, y2)| {
        (timing.easing_x1 - x1).abs() < 1e-6
            && (timing.easing_y1 - y1).abs() < 1e-6
            && (timing.easing_x2 - x2).abs() < 1e-6
            && (timing.easing_y2 - y2).abs() < 1e-6
    })
}

#[allow(dead_code)]
pub(super) fn matching_spring_preset(timing: MotionEffectTransformTiming) -> Option<usize> {
    SPRING_PRESETS
        .iter()
        .position(|(_, bounce)| (timing.spring_bounce - bounce).abs() < 1e-6)
}

/// Small curve preview for the timing buttons. The polyline is drawn from
/// the same `apply` the renderer uses, so the icon is literally the motion
/// it applies. A faint baseline marks the target (1.0): Ease touches it at
/// the end while a spring visibly crosses it, which keeps even a gentle
/// 5 % overshoot readable at icon size.
pub(super) fn timing_curve_icon(
    timing: MotionEffectTransformTiming,
    width: i32,
    height: i32,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_halign(Align::Center);
    area.set_valign(Align::Center);
    area.set_can_target(false);
    area.set_draw_func(move |area, context, width, height| {
        let color = area.style_context().color();
        let pad = 3.0_f64;
        let usable_w = (f64::from(width) - pad * 2.0).max(1.0);
        let usable_h = (f64::from(height) - pad * 2.0).max(1.0);
        const SAMPLES: usize = 48;
        let values: Vec<f64> = (0..=SAMPLES)
            .map(|index| timing.apply(index as f64 / SAMPLES as f64))
            .collect();
        let top = values
            .iter()
            .fold(1.0_f64, |peak, value| peak.max(*value))
            .max(0.001);
        let point = |t: f64, value: f64| (pad + t * usable_w, pad + (1.0 - value / top) * usable_h);
        // ponytail: dashed baseline, springs cross it while ease only touches it
        let baseline_y = pad + (1.0 - 1.0 / top) * usable_h;
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            f64::from(color.alpha()) * 0.35,
        );
        context.set_line_width(1.0);
        context.set_dash(&[2.0, 2.0], 0.0);
        context.move_to(pad, baseline_y);
        context.line_to(pad + usable_w, baseline_y);
        context.stroke().ok();
        context.set_dash(&[], 0.0);
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            f64::from(color.alpha()) * 0.9,
        );
        context.set_line_width(1.6);
        context.set_line_cap(LineCap::Round);
        context.set_line_join(gtk4::cairo::LineJoin::Round);
        for (index, value) in values.iter().enumerate() {
            let (x, y) = point(index as f64 / SAMPLES as f64, *value);
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        context.stroke().ok();
    });
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_presets_match_their_curves() {
        let base = MotionEffectTransformTiming::default();
        for (index, _) in EASE_PRESETS.iter().enumerate() {
            assert_eq!(
                matching_ease_preset(ease_preset_timing(index, base)),
                Some(index)
            );
        }
        for (index, _) in SPRING_PRESETS.iter().enumerate() {
            assert_eq!(
                matching_spring_preset(spring_preset_timing(index, base)),
                Some(index)
            );
        }
        let custom = MotionEffectTransformTiming {
            easing_x1: 0.1,
            easing_y1: 0.2,
            ..base
        };
        assert_eq!(matching_ease_preset(custom), None);
    }
}
