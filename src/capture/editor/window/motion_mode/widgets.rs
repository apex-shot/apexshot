use gtk4::{
    gdk, prelude::*, Align, Box as GtkBox, ColorChooserWidget, DrawingArea, Entry, Label,
    MenuButton, Orientation, Overlay, Popover,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::{
    MAX_MOTION_POS, MAX_MOTION_YAW, MIN_MOTION_POS, MIN_MOTION_YAW,
};
use crate::recording::editor::window::tool_sidebar::FillSlider;

pub(super) fn motion_color_control(
    initial: gdk::RGBA,
    tooltip: &str,
    on_changed: impl Fn(gdk::RGBA) + 'static,
) -> GtkBox {
    let on_changed = Rc::new(on_changed);
    let trigger = MenuButton::new();
    trigger.set_size_request(42, 42);
    trigger.set_halign(Align::Start);
    trigger.set_hexpand(false);
    trigger.set_tooltip_text(Some(&t(tooltip)));
    trigger.add_css_class("editor-motion-color-button");

    let swatch = DrawingArea::new();
    swatch.set_content_width(32);
    swatch.set_content_height(32);
    swatch.set_can_target(false);
    swatch.set_halign(Align::Center);
    swatch.set_valign(Align::Center);
    swatch.add_css_class("editor-motion-color-swatch");
    let selected = Rc::new(RefCell::new(initial));
    swatch.set_draw_func({
        let selected = selected.clone();
        move |_area, context, width, height| {
            let rgba = selected.borrow();
            context.set_source_rgba(
                rgba.red().into(),
                rgba.green().into(),
                rgba.blue().into(),
                rgba.alpha().into(),
            );
            context.rectangle(0.0, 0.0, width as f64, height as f64);
            context.fill().ok();
        }
    });
    let chooser = ColorChooserWidget::new();
    chooser.set_use_alpha(true);
    chooser.set_rgba(&initial);
    chooser.set_size_request(260, 260);
    chooser.set_hexpand(false);
    chooser.set_vexpand(false);
    chooser.add_css_class("editor-motion-color-chooser");
    let hex_entry = Entry::new();
    hex_entry.set_width_chars(9);
    hex_entry.set_text(&motion_color_hex(initial));
    hex_entry.set_tooltip_text(Some(&t("Hex color")));
    hex_entry.add_css_class("editor-motion-color-hex-entry");

    chooser.connect_rgba_notify({
        let hex_entry = hex_entry.clone();
        let on_changed = on_changed.clone();
        let selected = selected.clone();
        let swatch = swatch.clone();
        move |chooser| {
            let rgba = chooser.rgba();
            *selected.borrow_mut() = rgba;
            swatch.queue_draw();
            let hex = motion_color_hex(rgba);
            if hex_entry.text().as_str() != hex {
                hex_entry.set_text(&hex);
            }
            on_changed(rgba);
        }
    });
    hex_entry.connect_changed({
        let chooser = chooser.clone();
        move |entry| {
            if let Some(rgba) = motion_color_from_hex(entry.text().as_str()) {
                if chooser.rgba() != rgba {
                    chooser.set_rgba(&rgba);
                }
            }
        }
    });

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("editor-motion-color-popover-body");
    body.set_width_request(280);
    body.append(&chooser);

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_offset(0, 4);
    popover.add_css_class("editor-motion-color-popover");
    popover.set_child(Some(&body));
    trigger.set_popover(Some(&popover));
    let trigger_host = Overlay::new();
    trigger_host.set_size_request(42, 42);
    trigger_host.set_halign(Align::Start);
    trigger_host.set_hexpand(false);
    trigger_host.set_child(Some(&trigger));
    trigger_host.add_overlay(&swatch);

    let control = GtkBox::new(Orientation::Horizontal, 8);
    control.add_css_class("editor-motion-color-control");
    control.append(&trigger_host);
    control.append(&hex_entry);
    control
}

pub(super) fn motion_color_hex(rgba: gdk::RGBA) -> String {
    let component = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (
        component(rgba.red()),
        component(rgba.green()),
        component(rgba.blue()),
        component(rgba.alpha()),
    );
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

pub(super) fn motion_color_from_hex(value: &str) -> Option<gdk::RGBA> {
    let value = value.trim().trim_start_matches('#');
    if !matches!(value.len(), 6 | 8) {
        return None;
    }
    let byte = |range: std::ops::Range<usize>| u8::from_str_radix(&value[range], 16).ok();
    let (r, g, b) = (byte(0..2)?, byte(2..4)?, byte(4..6)?);
    let a = if value.len() == 8 { byte(6..8)? } else { 255 };
    Some(gdk::RGBA::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ))
}

pub(super) fn motion_rgba([red, green, blue, alpha]: [f64; 4]) -> gdk::RGBA {
    gdk::RGBA::new(red as f32, green as f32, blue as f32, alpha as f32)
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

pub(super) fn percent_slider_row(title: &str, initial: f64) -> (GtkBox, Label, FillSlider) {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let value = Label::new(Some(&format!("{:.0}%", initial * 100.0)));
    header.set_visible(false);
    let slider =
        FillSlider::new_with_value_text(title, |value, _, _| format!("{:.0}%", value * 100.0));
    slider.set_range(MIN_MOTION_POS, MAX_MOTION_POS);
    slider.set_increments(0.01, 0.1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_color_hex_input_round_trips_rgb_and_alpha() {
        let opaque = motion_color_from_hex("#FE8040").expect("opaque hex parses");
        assert_eq!(motion_color_hex(opaque), "#FE8040");
        let translucent = motion_color_from_hex("80A0C040").expect("rgba hex parses");
        assert_eq!(motion_color_hex(translucent), "#80A0C040");
        assert!(motion_color_from_hex("#bad").is_none());
    }
}
