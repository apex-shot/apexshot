//! Multi-monitor display picker for the Rust GTK overlay.
//!
//! Mirrors the C++ `MonitorPicker` flow:
//! - 1 monitor → that monitor immediately (no UI)
//! - multi → floating "Select a display" panel
//! - Esc / Cancel → cancelled
//! - number keys 1–9 select by sorted left-to-right order
//!
//! The picker is metadata-only: freeze/capture happens after dismiss so the
//! panel never appears in the frozen background.

use super::api::SelectionError;
use gtk4::{
    cairo, gdk, glib, prelude::*, Align, Box as GtkBox, Button, CssProvider, DrawingArea,
    EventControllerKey, EventControllerMotion, GestureClick, Label, Orientation, Window,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use crate::i18n::{t, tfmt};

/// Metadata about a connected monitor (for logging / capture targeting).
#[derive(Debug, Clone)]
pub struct MonitorChoice {
    pub index: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub connector: String,
    pub is_primary: bool,
}

impl MonitorChoice {
    pub fn from_monitor(index: u32, monitor: &gdk::Monitor, is_primary: bool) -> Self {
        let geometry = monitor.geometry();
        let connector = monitor
            .connector()
            .map(|s| s.to_string())
            .or_else(|| monitor.model().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                let number = (index + 1).to_string();
                tfmt("Display {number}", &[("number", &number)])
            });
        Self {
            index,
            x: geometry.x(),
            y: geometry.y(),
            width: geometry.width(),
            height: geometry.height(),
            connector,
            is_primary,
        }
    }
}

/// Collect every `gdk::Monitor` currently attached to `display`.
pub fn list_monitors(display: &gdk::Display) -> Vec<gdk::Monitor> {
    let model = display.monitors();
    let n = model.n_items();
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        if let Some(obj) = model.item(i) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                out.push(monitor);
            }
        }
    }
    out
}

/// Sort monitor indices left-to-right (then top-to-bottom) for stable numbering.
fn sorted_monitor_indices(monitors: &[gdk::Monitor]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..monitors.len()).collect();
    indices.sort_by(|&a, &b| {
        let ga = monitors[a].geometry();
        let gb = monitors[b].geometry();
        ga.x()
            .cmp(&gb.x())
            .then_with(|| ga.y().cmp(&gb.y()))
            .then_with(|| a.cmp(&b))
    });
    indices
}

/// Best-effort "primary" flag: monitor whose origin is closest to (0, 0).
fn is_primary_guess(monitors: &[gdk::Monitor], index: usize) -> bool {
    let Some(best) = monitors
        .iter()
        .enumerate()
        .min_by_key(|(_, m)| {
            let g = m.geometry();
            (g.x().unsigned_abs() + g.y().unsigned_abs(), g.x(), g.y())
        })
        .map(|(i, _)| i)
    else {
        return false;
    };
    best == index
}

/// Resolve the target monitor for area capture (C++ `selectTargetScreen`).
///
/// - 0 monitors → error
/// - 1 monitor → that monitor (no UI)
/// - multi → interactive floating picker
///
/// Cancelled when the user presses Esc / clicks Cancel / closes the panel.
pub fn select_target_monitor() -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    let display = gdk::Display::default()
        .ok_or_else(|| SelectionError::InitError("No display found for monitor picker".into()))?;

    let monitors = list_monitors(&display);
    if monitors.is_empty() {
        return Err(SelectionError::InitError("No monitor found".into()));
    }

    if monitors.len() == 1 {
        let choice = MonitorChoice::from_monitor(0, &monitors[0], true);
        eprintln!(
            "[monitor-picker] single display — skipping picker ({} {}×{}+{}+{})",
            choice.connector, choice.width, choice.height, choice.x, choice.y
        );
        return Ok((monitors[0].clone(), choice));
    }

    select_monitor_interactive(&display, &monitors)
}

#[derive(Clone)]
struct PickerCard {
    monitor_index: i32,
    display_number: usize,
    choice: MonitorChoice,
}

const PICKER_MARGIN_X: f64 = 36.0;
const PICKER_CARD_W: f64 = 288.0;
const PICKER_CARD_H: f64 = 240.0;
const PICKER_CARD_GAP: f64 = 16.0;
const PICKER_PREVIEW_W: f64 = 260.0;
const PICKER_PREVIEW_H: f64 = 156.0;
const PICKER_CARD_PAD: f64 = 14.0;

fn picker_panel_size(card_count: usize) -> (i32, i32) {
    let width = (PICKER_MARGIN_X * 2.0
        + card_count as f64 * PICKER_CARD_W
        + card_count.saturating_sub(1) as f64 * PICKER_CARD_GAP) as i32;
    // 28 top + title 22 + 4 + hint 15 + 22 + cards 240 + 12 + cancel 34 + 20.
    (width.max(360), 397)
}

fn picker_card_rect(index: usize) -> PickerRect {
    PickerRect {
        x: PICKER_MARGIN_X + index as f64 * (PICKER_CARD_W + PICKER_CARD_GAP),
        y: 91.0,
        width: PICKER_CARD_W,
        height: PICKER_CARD_H,
    }
}

#[derive(Clone, Copy)]
struct PickerRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl PickerRect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

fn picker_round_rect(context: &cairo::Context, rect: PickerRect, radius: f64) {
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(
        rect.x + rect.width - radius,
        rect.y + radius,
        radius,
        -PI / 2.0,
        0.0,
    );
    context.arc(
        rect.x + rect.width - radius,
        rect.y + rect.height - radius,
        radius,
        0.0,
        PI / 2.0,
    );
    context.arc(
        rect.x + radius,
        rect.y + rect.height - radius,
        radius,
        PI / 2.0,
        PI,
    );
    context.arc(rect.x + radius, rect.y + radius, radius, PI, PI * 1.5);
    context.close_path();
}

fn picker_text(
    context: &cairo::Context,
    rect: PickerRect,
    text: &str,
    size: f64,
    weight: cairo::FontWeight,
    color: (f64, f64, f64, f64),
    centered: bool,
) {
    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        cairo::FontSlant::Normal,
        weight,
    );
    context.set_font_size(size);
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    if let Ok(extents) = context.text_extents(text) {
        let x = if centered {
            rect.x + (rect.width - extents.width()) / 2.0 - extents.x_bearing()
        } else {
            rect.x - extents.x_bearing()
        };
        let y = rect.y + (rect.height - extents.height()) / 2.0 - extents.y_bearing();
        context.move_to(x, y);
        let _ = context.show_text(text);
    }
}

fn picker_elide(context: &cairo::Context, text: &str, max_width: f64) -> String {
    if context
        .text_extents(text)
        .is_ok_and(|m| m.width() <= max_width)
    {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    while !out.is_empty() {
        out.pop();
        let candidate = format!("{out}…");
        if context
            .text_extents(&candidate)
            .is_ok_and(|m| m.width() <= max_width)
        {
            return candidate;
        }
    }
    "…".to_owned()
}

fn draw_display_glyph(context: &cairo::Context, rect: PickerRect) {
    picker_round_rect(context, rect, 10.0);
    context.set_source_rgb(5.0 / 255.0, 5.0 / 255.0, 7.0 / 255.0);
    let _ = context.fill();

    let screen = PickerRect {
        x: rect.x + 26.0,
        y: rect.y + 20.0,
        width: PICKER_PREVIEW_W - 52.0,
        height: PICKER_PREVIEW_H - 54.0,
    };
    picker_round_rect(context, screen, 8.0);
    context.set_source_rgb(25.0 / 255.0, 25.0 / 255.0, 28.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
    context.set_line_width(1.2);
    let _ = context.stroke();

    let inner = PickerRect {
        x: screen.x + 8.0,
        y: screen.y + 8.0,
        width: screen.width - 16.0,
        height: screen.height - 16.0,
    };
    let gradient = cairo::LinearGradient::new(
        inner.x,
        inner.y,
        inner.x + inner.width,
        inner.y + inner.height,
    );
    gradient.add_color_stop_rgb(0.0, 38.0 / 255.0, 38.0 / 255.0, 42.0 / 255.0);
    gradient.add_color_stop_rgb(1.0, 24.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0);
    let _ = context.set_source(&gradient);
    context.rectangle(inner.x, inner.y, inner.width, inner.height);
    let _ = context.fill();
    context.set_source_rgba(1.0, 102.0 / 255.0, 0.0, 90.0 / 255.0);
    context.rectangle(inner.x + 10.0, inner.y + 14.0, inner.width * 0.45, 8.0);
    let _ = context.fill();
    context.set_source_rgba(1.0, 1.0, 1.0, 28.0 / 255.0);
    context.rectangle(inner.x + 10.0, inner.y + 30.0, inner.width * 0.7, 6.0);
    let _ = context.fill();
    context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
    context.rectangle(inner.x + 10.0, inner.y + 44.0, inner.width * 0.55, 6.0);
    let _ = context.fill();

    let stand_top = screen.y + screen.height + 4.0;
    context.set_source_rgb(38.0 / 255.0, 38.0 / 255.0, 42.0 / 255.0);
    picker_round_rect(
        context,
        PickerRect {
            x: rect.x + PICKER_PREVIEW_W / 2.0 - 14.0,
            y: stand_top,
            width: 28.0,
            height: 8.0,
        },
        2.0,
    );
    let _ = context.fill();
    picker_round_rect(
        context,
        PickerRect {
            x: rect.x + PICKER_PREVIEW_W / 2.0 - 36.0,
            y: stand_top + 8.0,
            width: 72.0,
            height: 5.0,
        },
        2.0,
    );
    let _ = context.fill();
}

fn draw_picker_card(
    context: &cairo::Context,
    index: usize,
    card: &PickerCard,
    hovered: bool,
    pressed: bool,
) {
    let raw = picker_card_rect(index);
    let outer = PickerRect {
        x: raw.x + 1.5,
        y: raw.y + 1.5,
        width: raw.width - 3.0,
        height: raw.height - 3.0,
    };
    picker_round_rect(
        context,
        PickerRect {
            y: outer.y + 3.0,
            ..outer
        },
        14.0,
    );
    context.set_source_rgba(
        0.0,
        0.0,
        0.0,
        if hovered { 80.0 / 255.0 } else { 40.0 / 255.0 },
    );
    let _ = context.fill();
    picker_round_rect(context, outer, 14.0);
    if hovered {
        context.set_source_rgba(1.0, 102.0 / 255.0, 0.0, 210.0 / 255.0);
    } else {
        context.set_source_rgb(25.0 / 255.0, 25.0 / 255.0, 28.0 / 255.0);
    }
    let _ = context.fill_preserve();
    context.set_source_rgba(
        1.0,
        if hovered { 102.0 / 255.0 } else { 1.0 },
        if hovered { 0.0 } else { 1.0 },
        if hovered { 200.0 / 255.0 } else { 30.0 / 255.0 },
    );
    context.set_line_width(if hovered { 1.6 } else { 1.0 });
    let _ = context.stroke();

    let inset = if pressed { 2.0 } else { 0.0 };
    let content = PickerRect {
        x: outer.x + PICKER_CARD_PAD + inset,
        y: outer.y + PICKER_CARD_PAD + inset,
        width: outer.width - (PICKER_CARD_PAD + inset) * 2.0,
        height: outer.height - (PICKER_CARD_PAD + inset) * 2.0,
    };
    let glyph = PickerRect {
        x: content.x,
        y: content.y,
        width: content.width,
        height: PICKER_PREVIEW_H,
    };
    draw_display_glyph(context, glyph);

    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    context.set_font_size(11.0);
    let number = card.display_number.to_string();
    let badge_w = context
        .text_extents(&number)
        .map(|m| (m.width() + 12.0).max(22.0))
        .unwrap_or(22.0);
    let badge = PickerRect {
        x: glyph.x + 8.0,
        y: glyph.y + 8.0,
        width: badge_w,
        height: 20.0,
    };
    picker_round_rect(context, badge, 6.0);
    if hovered {
        context.set_source_rgb(1.0, 102.0 / 255.0, 0.0);
    } else {
        context.set_source_rgba(0.0, 0.0, 0.0, 180.0 / 255.0);
    }
    let _ = context.fill();
    picker_text(
        context,
        badge,
        &number,
        11.0,
        cairo::FontWeight::Bold,
        (1.0, 1.0, 1.0, 1.0),
        true,
    );

    if card.choice.is_primary {
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            cairo::FontSlant::Normal,
            cairo::FontWeight::Normal,
        );
        context.set_font_size(10.0);
        let chip_text = "Primary";
        let chip_w = context
            .text_extents(chip_text)
            .map(|m| m.width() + 12.0)
            .unwrap_or(48.0);
        let chip = PickerRect {
            x: glyph.x + glyph.width - chip_w - 8.0,
            y: glyph.y + 8.0,
            width: chip_w,
            height: 18.0,
        };
        picker_round_rect(context, chip, 5.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 22.0 / 255.0);
        let _ = context.fill();
        picker_text(
            context,
            chip,
            chip_text,
            10.0,
            cairo::FontWeight::Normal,
            (1.0, 1.0, 1.0, 200.0 / 255.0),
            true,
        );
    }

    let meta_y = glyph.y + glyph.height + 12.0;
    picker_text(
        context,
        PickerRect {
            x: content.x,
            y: meta_y,
            width: content.width,
            height: 18.0,
        },
        &format!("Display {}", card.display_number),
        13.0,
        cairo::FontWeight::Bold,
        (1.0, 1.0, 1.0, 240.0 / 255.0),
        false,
    );
    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    context.set_font_size(11.0);
    let metadata = format!(
        "{} × {}  ·  {}",
        card.choice.width, card.choice.height, card.choice.connector
    );
    let metadata = picker_elide(context, &metadata, content.width);
    picker_text(
        context,
        PickerRect {
            x: content.x,
            y: meta_y + 18.0,
            width: content.width,
            height: 16.0,
        },
        &metadata,
        11.0,
        cairo::FontWeight::Normal,
        (1.0, 1.0, 1.0, 150.0 / 255.0),
        false,
    );
}

fn cancel_rect(panel_width: f64) -> PickerRect {
    PickerRect {
        x: panel_width / 2.0 - 39.0,
        y: 343.0,
        width: 78.0,
        height: 34.0,
    }
}

fn draw_picker_panel(
    context: &cairo::Context,
    width: i32,
    height: i32,
    cards: &[PickerCard],
    hovered: i32,
    pressed: i32,
    cancel_hovered: bool,
) {
    let panel = PickerRect {
        x: 1.0,
        y: 1.0,
        width: width as f64 - 2.0,
        height: height as f64 - 2.0,
    };
    picker_round_rect(context, panel, 18.0);
    context.set_source_rgb(5.0 / 255.0, 5.0 / 255.0, 7.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 24.0 / 255.0);
    context.set_line_width(1.0);
    let _ = context.stroke();
    context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
    context.move_to(panel.x + 22.0, panel.y + 1.0);
    context.line_to(panel.x + panel.width - 22.0, panel.y + 1.0);
    let _ = context.stroke();

    picker_text(
        context,
        PickerRect {
            x: 0.0,
            y: 28.0,
            width: width as f64,
            height: 22.0,
        },
        "Select a display",
        18.0,
        cairo::FontWeight::Bold,
        (1.0, 1.0, 1.0, 240.0 / 255.0),
        true,
    );
    let hint = format!(
        "Click a display  ·  Esc to cancel  ·  1–{}",
        cards.len().min(9)
    );
    picker_text(
        context,
        PickerRect {
            x: 0.0,
            y: 54.0,
            width: width as f64,
            height: 15.0,
        },
        &hint,
        12.0,
        cairo::FontWeight::Normal,
        (1.0, 1.0, 1.0, 128.0 / 255.0),
        true,
    );
    for (index, card) in cards.iter().enumerate() {
        draw_picker_card(
            context,
            index,
            card,
            hovered == index as i32,
            pressed == index as i32,
        );
    }
    let cancel = cancel_rect(width as f64);
    picker_round_rect(context, cancel, 8.0);
    context.set_source_rgba(
        1.0,
        1.0,
        1.0,
        if cancel_hovered {
            28.0 / 255.0
        } else {
            16.0 / 255.0
        },
    );
    let _ = context.fill();
    picker_text(
        context,
        cancel,
        "Cancel",
        12.0,
        cairo::FontWeight::Bold,
        (
            1.0,
            1.0,
            1.0,
            if cancel_hovered {
                245.0 / 255.0
            } else {
                209.0 / 255.0
            },
        ),
        true,
    );
}

fn select_monitor_interactive(
    display: &gdk::Display,
    monitors: &[gdk::Monitor],
) -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    install_picker_canvas_css(display);
    let sorted = sorted_monitor_indices(monitors);
    let cards: Vec<_> = sorted
        .iter()
        .enumerate()
        .map(|(display_number, &monitor_index)| PickerCard {
            monitor_index: monitor_index as i32,
            display_number: display_number + 1,
            choice: MonitorChoice::from_monitor(
                monitor_index as u32,
                &monitors[monitor_index],
                is_primary_guess(monitors, monitor_index),
            ),
        })
        .collect();
    let (panel_width, panel_height) = picker_panel_size(cards.len());
    let result = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);
    let panel = Window::builder()
        .title("ApexShot Display Picker")
        .decorated(false)
        .resizable(false)
        .modal(true)
        .css_classes(["apexshot-monitor-picker-canvas"])
        .build();
    panel.set_default_size(panel_width, panel_height);

    let hovered = Rc::new(Cell::new(0));
    let pressed = Rc::new(Cell::new(-1));
    let cancel_hovered = Rc::new(Cell::new(false));
    let drawing = DrawingArea::new();
    drawing.set_content_width(panel_width);
    drawing.set_content_height(panel_height);
    drawing.set_size_request(panel_width, panel_height);
    drawing.set_focusable(true);
    {
        let cards = cards.clone();
        let hovered = hovered.clone();
        let pressed = pressed.clone();
        let cancel_hovered = cancel_hovered.clone();
        drawing.set_draw_func(move |_, context, width, height| {
            draw_picker_panel(
                context,
                width,
                height,
                &cards,
                hovered.get(),
                pressed.get(),
                cancel_hovered.get(),
            );
        });
    }

    let card_hit = {
        let card_count = cards.len();
        move |x: f64, y: f64| {
            (0..card_count)
                .find(|&index| picker_card_rect(index).contains(x, y))
                .map(|index| index as i32)
        }
    };
    let motion = EventControllerMotion::new();
    {
        let hovered = hovered.clone();
        let cancel_hovered = cancel_hovered.clone();
        let drawing = drawing.clone();
        motion.connect_motion(move |_, x, y| {
            let next = card_hit(x, y).unwrap_or(-1);
            let next_cancel = cancel_rect(panel_width as f64).contains(x, y);
            if hovered.replace(next) != next || cancel_hovered.replace(next_cancel) != next_cancel {
                drawing.queue_draw();
            }
        });
    }
    {
        let hovered = hovered.clone();
        let cancel_hovered = cancel_hovered.clone();
        let drawing = drawing.clone();
        motion.connect_leave(move |_| {
            if hovered.replace(-1) != -1 || cancel_hovered.replace(false) {
                drawing.queue_draw();
            }
        });
    }
    drawing.add_controller(motion);
    let click = GestureClick::builder().button(1).build();
    {
        let pressed = pressed.clone();
        let drawing = drawing.clone();
        click.connect_pressed(move |_, _, x, y| {
            pressed.set(card_hit(x, y).unwrap_or(-1));
            drawing.queue_draw();
        });
    }
    {
        let cards = cards.clone();
        let result = result.clone();
        let main_loop = main_loop.clone();
        let panel = panel.clone();
        let pressed = pressed.clone();
        let drawing = drawing.clone();
        click.connect_released(move |_, _, x, y| {
            let hit = card_hit(x, y);
            let pressed_index = pressed.replace(-1);
            if hit == Some(pressed_index) && pressed_index >= 0 {
                *result.borrow_mut() = Some(cards[pressed_index as usize].monitor_index);
                panel.close();
                main_loop.quit();
                return;
            }
            if cancel_rect(panel_width as f64).contains(x, y) {
                *result.borrow_mut() = Some(-1);
                panel.close();
                main_loop.quit();
                return;
            }
            drawing.queue_draw();
        });
    }
    drawing.add_controller(click);
    panel.set_child(Some(&drawing));

    let key = EventControllerKey::new();
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        let panel = panel.clone();
        let cards = cards.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            use gtk4::gdk::Key;
            let chosen = match keyval {
                Key::_1 => cards.first().map(|card| card.monitor_index),
                Key::_2 => cards.get(1).map(|card| card.monitor_index),
                Key::_3 => cards.get(2).map(|card| card.monitor_index),
                Key::_4 => cards.get(3).map(|card| card.monitor_index),
                Key::_5 => cards.get(4).map(|card| card.monitor_index),
                Key::_6 => cards.get(5).map(|card| card.monitor_index),
                Key::_7 => cards.get(6).map(|card| card.monitor_index),
                Key::_8 => cards.get(7).map(|card| card.monitor_index),
                Key::_9 => cards.get(8).map(|card| card.monitor_index),
                Key::Escape => Some(-1),
                Key::Return | Key::KP_Enter | Key::space => {
                    cards.first().map(|card| card.monitor_index)
                }
                _ => None,
            };
            if let Some(chosen) = chosen {
                *result.borrow_mut() = Some(chosen);
                panel.close();
                main_loop.quit();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    panel.add_controller(key);
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        panel.connect_close_request(move |_| {
            if result.borrow().is_none() {
                *result.borrow_mut() = Some(-1);
            }
            main_loop.quit();
            glib::Propagation::Proceed
        });
    }
    panel.present();
    let _ = drawing.grab_focus();
    main_loop.run();
    panel.hide();
    panel.destroy();
    while glib::MainContext::default().iteration(false) {}
    std::thread::sleep(std::time::Duration::from_millis(120));
    while glib::MainContext::default().iteration(false) {}
    let code = result.borrow().unwrap_or(-1);
    if code < 0 || code as usize >= monitors.len() {
        eprintln!("[monitor-picker] cancelled");
        return Err(SelectionError::Cancelled);
    }
    let monitor = monitors[code as usize].clone();
    let choice = MonitorChoice::from_monitor(
        code as u32,
        &monitor,
        is_primary_guess(monitors, code as usize),
    );
    eprintln!(
        "[monitor-picker] selected index={} name={} geom={}x{}+{}+{}",
        choice.index, choice.connector, choice.width, choice.height, choice.x, choice.y
    );
    Ok((monitor, choice))
}

fn select_monitor_interactive_legacy(
    display: &gdk::Display,
    monitors: &[gdk::Monitor],
) -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    install_picker_css(display);

    let result: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);

    // Transient floating panel — does not cover the desktop (live desktop stays).
    // Compositors place the modal on the active/focused display.
    let panel = Window::builder()
        .title(t("Select a display"))
        .decorated(false)
        .resizable(false)
        .modal(true)
        .css_classes(["apexshot-monitor-picker"])
        .build();
    panel.set_default_size(-1, -1);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_margin_top(28);
    root.set_margin_bottom(20);
    root.set_margin_start(36);
    root.set_margin_end(36);
    root.add_css_class("apexshot-monitor-picker-root");

    let title = Label::new(Some(&t("Select a display")));
    title.add_css_class("apexshot-monitor-picker-title");
    title.set_halign(Align::Center);
    root.append(&title);

    let n_keys = monitors.len().min(9);
    let key_range = format!("1–{n_keys}");
    let hint = Label::new(Some(&tfmt(
        "Click a display · Esc to cancel · {keys}",
        &[("keys", &key_range)],
    )));
    hint.add_css_class("apexshot-monitor-picker-hint");
    hint.set_halign(Align::Center);
    hint.set_margin_top(4);
    hint.set_margin_bottom(22);
    root.append(&hint);

    let row = GtkBox::new(Orientation::Horizontal, 16);
    row.set_halign(Align::Center);

    let sorted = sorted_monitor_indices(monitors);
    let mut first_card: Option<Button> = None;

    for (display_ord, &mon_index) in sorted.iter().enumerate() {
        let monitor = &monitors[mon_index];
        let primary = is_primary_guess(monitors, mon_index);
        let choice = MonitorChoice::from_monitor(mon_index as u32, monitor, primary);
        let card = build_monitor_card(display_ord, &choice);

        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        let mon_index_i = mon_index as i32;
        card.connect_clicked(move |_| {
            *result_c.borrow_mut() = Some(mon_index_i);
            panel_c.close();
            loop_c.quit();
        });

        row.append(&card);
        if first_card.is_none() {
            first_card = Some(card);
        }
    }
    root.append(&row);

    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("apexshot-monitor-picker-cancel");
    cancel.set_halign(Align::Center);
    cancel.set_margin_top(12);
    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        cancel.connect_clicked(move |_| {
            *result_c.borrow_mut() = Some(-1);
            panel_c.close();
            loop_c.quit();
        });
    }
    root.append(&cancel);

    panel.set_child(Some(&root));

    // Keyboard: Esc cancel, 1–9 select by sorted order.
    let key = EventControllerKey::new();
    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        let sorted_keys = sorted.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            use gtk4::gdk::Key;
            if keyval == Key::Escape {
                *result_c.borrow_mut() = Some(-1);
                panel_c.close();
                loop_c.quit();
                return glib::Propagation::Stop;
            }
            // GDK keyvals for '1'..'9'
            let digit = match keyval {
                Key::_1 => Some(0usize),
                Key::_2 => Some(1),
                Key::_3 => Some(2),
                Key::_4 => Some(3),
                Key::_5 => Some(4),
                Key::_6 => Some(5),
                Key::_7 => Some(6),
                Key::_8 => Some(7),
                Key::_9 => Some(8),
                _ => None,
            };
            if let Some(ord) = digit {
                if let Some(&mon_index) = sorted_keys.get(ord) {
                    *result_c.borrow_mut() = Some(mon_index as i32);
                    panel_c.close();
                    loop_c.quit();
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }
    panel.add_controller(key);

    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        panel.connect_close_request(move |_| {
            if result_c.borrow().is_none() {
                *result_c.borrow_mut() = Some(-1);
            }
            loop_c.quit();
            glib::Propagation::Proceed
        });
    }

    // Size hint so the panel lays out cards before present().
    let panel_w = estimate_panel_width(monitors.len());
    panel.set_default_size(panel_w, -1);

    panel.present();
    if let Some(card) = first_card {
        card.grab_focus();
    }

    main_loop.run();

    // Force-destroy and drain so the surface is unmapped before freeze/capture.
    panel.hide();
    panel.destroy();
    // Process pending unmap events.
    while glib::MainContext::default().iteration(false) {}
    std::thread::sleep(std::time::Duration::from_millis(120));
    while glib::MainContext::default().iteration(false) {}

    let code = result.borrow().unwrap_or(-1);
    if code < 0 || code as usize >= monitors.len() {
        eprintln!("[monitor-picker] cancelled");
        return Err(SelectionError::Cancelled);
    }

    let monitor = monitors[code as usize].clone();
    let choice = MonitorChoice::from_monitor(
        code as u32,
        &monitor,
        is_primary_guess(monitors, code as usize),
    );
    eprintln!(
        "[monitor-picker] selected index={} name={} geom={}x{}+{}+{}",
        choice.index, choice.connector, choice.width, choice.height, choice.x, choice.y
    );
    Ok((monitor, choice))
}

fn estimate_panel_width(n_monitors: usize) -> i32 {
    // Card ~288px + gaps + margins (mirrors C++ kPreviewW + pad).
    let card = 288;
    let gap = 16;
    let margins = 72;
    (n_monitors as i32 * card + (n_monitors as i32 - 1).max(0) * gap + margins).max(360)
}

fn build_monitor_card(display_ord: usize, choice: &MonitorChoice) -> Button {
    let card = Button::new();
    card.add_css_class("apexshot-monitor-card");
    card.set_focusable(true);
    card.set_can_focus(true);

    let col = GtkBox::new(Orientation::Vertical, 8);
    col.set_margin_top(14);
    col.set_margin_bottom(14);
    col.set_margin_start(14);
    col.set_margin_end(14);
    col.set_size_request(260, -1);

    // Preview glyph area (stylized monitor shape via CSS + labels).
    let glyph = GtkBox::new(Orientation::Vertical, 0);
    glyph.add_css_class("apexshot-monitor-glyph");
    glyph.set_size_request(260, 140);
    glyph.set_halign(Align::Fill);

    let badge_row = GtkBox::new(Orientation::Horizontal, 0);
    badge_row.set_margin_top(8);
    badge_row.set_margin_start(8);
    badge_row.set_margin_end(8);
    badge_row.set_hexpand(true);

    let num = Label::new(Some(&(display_ord + 1).to_string()));
    num.add_css_class("apexshot-monitor-badge");
    num.set_halign(Align::Start);
    badge_row.append(&num);

    if choice.is_primary {
        let spacer = Label::new(None);
        spacer.set_hexpand(true);
        badge_row.append(&spacer);
        let primary = Label::new(Some(&t("Primary")));
        primary.add_css_class("apexshot-monitor-primary-chip");
        primary.set_halign(Align::End);
        badge_row.append(&primary);
    }

    glyph.append(&badge_row);

    let screen_fake = Label::new(None);
    screen_fake.set_vexpand(true);
    glyph.append(&screen_fake);

    col.append(&glyph);

    let display_number = (display_ord + 1).to_string();
    let title = Label::new(Some(&tfmt(
        "Display {number}",
        &[("number", &display_number)],
    )));
    title.add_css_class("apexshot-monitor-card-title");
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    col.append(&title);

    let sub = Label::new(Some(&format!(
        "{} × {}  ·  {}",
        choice.width, choice.height, choice.connector
    )));
    sub.add_css_class("apexshot-monitor-card-sub");
    sub.set_halign(Align::Start);
    sub.set_xalign(0.0);
    sub.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    col.append(&sub);

    card.set_child(Some(&col));
    card
}

fn install_picker_canvas_css(display: &gdk::Display) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let provider = CssProvider::new();
        provider.load_from_data(
            "window.apexshot-monitor-picker-canvas, window.apexshot-monitor-picker-canvas > * { background: transparent; }",
        );
        gtk4::style_context_add_provider_for_display(
            display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

fn install_picker_css(display: &gdk::Display) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let provider = CssProvider::new();
        provider.load_from_data(
            r#"
            window.apexshot-monitor-picker {
                background-color: #050507;
                border-radius: 20px;
                border: 1px solid rgba(255, 255, 255, 0.16);
            }
            .apexshot-monitor-picker-root {
                background-color: #050507;
            }
            .apexshot-monitor-picker-title {
                color: rgba(255, 255, 255, 0.95);
                font-size: 18px;
                font-weight: 600;
            }
            .apexshot-monitor-picker-hint {
                color: rgba(255, 255, 255, 0.5);
                font-size: 12px;
            }
            button.apexshot-monitor-card {
                background-color: #19191c;
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 14px;
                padding: 0;
                box-shadow: 0 3px 0 rgba(0, 0, 0, 0.25);
            }
            button.apexshot-monitor-card:hover,
            button.apexshot-monitor-card:focus {
                background-color: rgba(255, 102, 0, 0.82);
                border-color: rgba(255, 102, 0, 0.78);
                border-width: 1.5px;
            }
            .apexshot-monitor-glyph {
                background-color: #050507;
                border-radius: 10px;
                border: 1px solid rgba(255, 255, 255, 0.12);
            }
            .apexshot-monitor-badge {
                background-color: rgba(0, 0, 0, 0.7);
                color: white;
                font-size: 11px;
                font-weight: 600;
                padding: 2px 8px;
                border-radius: 6px;
                min-width: 22px;
            }
            button.apexshot-monitor-card:hover .apexshot-monitor-badge,
            button.apexshot-monitor-card:focus .apexshot-monitor-badge {
                background-color: #ff6600;
            }
            .apexshot-monitor-primary-chip {
                background-color: rgba(255, 255, 255, 0.09);
                color: rgba(255, 255, 255, 0.78);
                font-size: 10px;
                font-weight: 500;
                padding: 2px 8px;
                border-radius: 5px;
            }
            .apexshot-monitor-card-title {
                color: rgba(255, 255, 255, 0.94);
                font-size: 13px;
                font-weight: 600;
            }
            .apexshot-monitor-card-sub {
                color: rgba(255, 255, 255, 0.55);
                font-size: 11px;
            }
            button.apexshot-monitor-picker-cancel {
                background: transparent;
                border: none;
                color: rgba(255, 255, 255, 0.55);
                font-size: 12px;
                padding: 8px 16px;
                box-shadow: none;
            }
            button.apexshot-monitor-picker-cancel:hover {
                color: rgba(255, 255, 255, 0.9);
                background: transparent;
            }
            "#,
        );
        gtk4::style_context_add_provider_for_display(
            display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

/// Resolve a previously chosen monitor by matching geometry origin.
///
/// Used when the picker ran before the overlay Application started and we need
/// to re-bind the same logical output inside `setup_window`.
pub fn find_monitor_at(display: &gdk::Display, x: i32, y: i32) -> Option<gdk::Monitor> {
    let monitors = list_monitors(display);
    monitors
        .into_iter()
        .find(|m| {
            let g = m.geometry();
            g.x() == x && g.y() == y
        })
        .or_else(|| {
            // Nearest origin match (handles minor geometry drift).
            list_monitors(display).into_iter().min_by_key(|m| {
                let g = m.geometry();
                (g.x() - x).unsigned_abs() + (g.y() - y).unsigned_abs()
            })
        })
}

/// Public helper for callers that only need geometry (no GTK Window yet).
pub fn select_target_monitor_choice() -> Result<MonitorChoice, SelectionError> {
    select_target_monitor().map(|(_, choice)| choice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_panel_width_grows_with_monitors() {
        assert!(estimate_panel_width(2) > estimate_panel_width(1));
        assert!(estimate_panel_width(3) > estimate_panel_width(2));
    }

    #[test]
    fn monitor_choice_from_fields() {
        // Pure unit: struct construction without GDK.
        let c = MonitorChoice {
            index: 1,
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            connector: "DP-2".into(),
            is_primary: false,
        };
        assert_eq!(c.connector, "DP-2");
        assert_eq!(c.width, 2560);
    }
}
